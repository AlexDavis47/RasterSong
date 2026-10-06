# Node authoring

How nodes are defined, registered and tested. What nodes *do* (shared settings, conversions, channel handling) is in
[Node behavior](node-behavior.md); every node's ports and parameters are in the generated [node reference](nodes.md).

## Node Contract

A node is one file under `crates/rastersong-graph/src/nodes/<category>/` that holds everything about it. It
implements two traits: `NodeKind` (what the node *is*: the registry, menus, inspector, tests and docs read this)
and `Node` (what it *does* each frame).

```rust
pub trait NodeKind: Node + Sized + 'static {
    const KIND: &'static str;                 // type name in graph files
    const SPEC: NodeSpec;                     // label, category, description, ports, parameters
    const TEST_CONFIGS: &'static [&'static str] = &[];  // parameter sets for the property tests
    const BENCH: Option<&'static str> = None;           // parameter set for benchmarks
    fn new(params: &Params) -> Result<Self, String>;    // must succeed with all defaults
}

pub trait Node: Send {
    /// For source nodes, the name of the host-supplied signal they read ("video", "audio").
    fn source(&self) -> Option<&str> { None }

    /// Output layouts for the given input layouts, or an error if the inputs don't fit.
    /// Defaults to passing the main input's layout through.
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String>;

    /// Called once the graph is compiled and layouts are known. Allocate buffers here.
    fn prepare(&mut self, ctx: &PrepareContext) {}

    /// Process exactly one frame. Inputs are already rate-matched; outputs are pre-sized.
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]);

    /// Clear all internal state, as if no frame had ever been processed.
    fn reset(&mut self) {}

    /// Samples of delay this node adds (non-zero for nodes that need lookahead).
    fn latency(&self, ctx: &PrepareContext) -> usize { 0 }

    /// How many frames of history this node needs before its output is valid (stateful nodes): its real
    /// length, never clamped (`UNBOUNDED_WARMUP` if it never settles). The engine applies the project's limit.
    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 { 0 }
}
```

### Adding a node

1. Create `nodes/<category>/<name>.rs` (copy `effect/bitcrush.rs` for a stateless effect, `effect/delay.rs` for a
   stateful one).
2. Add one line to the `nodes!` list in `nodes/mod.rs`: `<name>: [<Type>]` under its category.

That is all. The add-node menu, the inspector, the registry tests, the property tests (block-size independence,
reset determinism, finite output, modulation sweeps), the benchmarks and `docs/nodes.md` are all driven by the
registry. In the node's file:

- **Parameters** are declared with `params!`, which gives `Type::PARAMS` and a named index per parameter
  (`Type::DRIVE`). Constructors read with `params.number_at(Self::DRIVE)`, `params.choice_as::<Shape>(Self::SHAPE)`
  and so on, and a compile-time assertion checks each constant is in the position of the parameter it names, so
  no parameter is ever looked up by a string or a bare `0`. Choices are enums made with `choice!`, which has no
  fallback arm. Every parameter, input and output has help text (a registry test fails without it).
- **Per-sample values** come from `ctx.value(Self::DRIVE, self.drive)`, which is the node's own constant when
  nothing modulates the parameter and the modulating signal's values when something does: `drive.at(i)`.
- **Ports** are in `SPEC` (`.inputs(&[...])`, `.outputs(&[...])`, with help text and wire-colour hints). A node
  defaults to one input `in` and one output `out`.
- **Tests** go in the same file: numeric tests with `crate::testing::{node, process_one}`, plus `TEST_CONFIGS`
  and `BENCH`. A registry test fails if an effect leaves them empty, so a new effect can't skip the property tests.
- The checks the registry makes at registration time (a duplicate type, more than `MAX_INPUTS` inputs or
  `MAX_PARAMS` parameters, a constructor that fails with the defaults) panic, and the registry tests run them for
  every built-in node.

Rules every node must satisfy (enforced by tests, see [Testing](testing.md)):

- **Deterministic:** the same inputs after `reset()` always give the same outputs.
- **Block-size independent:** a stateful node keeps its own history (e.g. a ring buffer). It never re-reads
  past samples from upstream, and never processes the same sample twice.
- **No allocation in `process`.**

### Shared building blocks

Shared code lives in `dsp.rs` (resampling, delay line, `mix`, dB conversion, `Biquad` with RBJ
low/high/band/all-pass, peak and shelf designs) and `nodes/support.rs` (`Unit`, `ms_to_samples`,
`settle_frames`, the conversion `Mapping`).

**Rule: before writing a helper in a node file, look here; if two nodes need it, it belongs here.** Per-node copies
of a filter, a detector, a unit list, a layout choice or a wet/dry mix are how implementations fragment. The
[DRY workstream](roadmap.md#code-health-and-dry) lists the known duplicates to remove.

## Parameters

Each node type is registered with a `NodeSpec`: a label, a category, a one-line description and a list of
`ParamSpec`s (name, label, help text, and a number range, choice list or text default). Constructors read their
parameters through those specs, so defaults and ranges live in one place, and the editor builds its parameter
panels from the same specs. A number has a usual range, which the slider shows, and hard limits, the values the
node can actually work with; unless a spec widens them, the limits are the usual range.

Soft bounds: typing (or dragging the value box) past the usual range, up to the node's limits, widens the slider to
match, as in Substance Designer.

### Parameter modulation

Any number parameter a spec doesn't mark `fixed` can be driven by a signal, connected like an input as
`"node.@param"`. The parameter's value is the base, and the signal moves it per sample:
`base + sweep × signal` (bipolar) or `base + sweep × |signal|` (unipolar, one way), clamped to the parameter's
limits. The modulation *amount* the user sets is a **percentage of the parameter's span** (`max − min` of its
usual range): `sweep = amount / 100 × span`, halved both ways so that 100% is the whole swing from the lowest
point to the highest (`ParamSpec::modulation_sweep`). A negative one-way amount turns the value down. Every parameter is
linear, frequencies included. A newly connected signal starts at 25%, one
way. Modulated values are kept within the usual range (widened to include the base value) unless the modulation
sets `overshoot`, when only the parameter's limits apply (`ParamSpec::modulation_bounds`). The compiler resamples the signal to the main input's length (with the node's interpolation
and latency compensation, like any secondary input) and hands the node the values through `ctx.param(i)`;
unmodulated parameters stay constants the node reads from its own fields, so they cost nothing.
`PrepareContext::modulation(i)` gives the range a modulated parameter can move over, for sizing buffers and warmup.
Specs also say which parameters show a pin on new nodes (`exposed`).

**Whole-number parameters** (counts, divisions, steps, seeds, pixel sizes) are declared `.integer()`. The usual range, default and limits must be whole (a test checks), the slider and value box snap to whole values, loaded fractional values are rounded by `migrate.rs`, and a modulated value is rounded at every sample. Parameters that merely accept fractions (Bit Crush bits) are not integers; the per-node `int` toggle covers those.

**Parameters marked `fixed`** can't be modulated. The spec's comment says they are "too costly or meaningless to
change while rendering", which in practice covers three different reasons (not warmup time):

1. *Structural:* changing the value changes what is allocated or how many things exist (chorus voices, phaser
   stages, Beat steps, Pack channels, Resample sizes, noise seed). These can never be modulated.
2. *Precomputed state:* the node builds buffers or coefficients once in `prepare` from the value (Three-Band Split
   crossovers, Reverb size, damping and pre-delay). Modulating them means redesigning the node's internals.
3. *Not done yet:* the node could take a per-sample value but nobody wrote it (Envelope attack and release, Slew
   rise and fall, Limiter release, Beat width and division, Oscillator phase, Chorus spread). The Phaser already
   shows how: it recomputes its coefficient per sample.

The inspector doesn't say which reason applies, which is the bug in the [roadmap](roadmap.md#node-settings-and-parameters).
The policy going forward is the opposite of protecting the user: a parameter is locked only when modulating it is
truly infeasible, and the lock carries a reason ([Decisions](decisions.md#modulation-is-allowed-unless-infeasible-october-2026)).

> **Under revision.** Hands-on testing found that some parameters are unmodulatable with no explanation, (a visible
> reason on locked parameters is [planned](roadmap.md#node-settings-and-parameters)). Percentage amounts and linear
> sliders have landed. Give frequency parameters a usual range that suits a linear slider (Cutoff is 0.01–200), since
> the range also sets the span modulation percentages are of.

Nodes only have their own inputs for signals that are part of what they do: Amplitude Modulation's modulator and
the compressor's and gate's sidechain. Delay, Bit Crush and Low Pass used to have a `modulation` input and a
`depth` parameter; parameter modulation replaced them, and graphs that still use them are upgraded when loaded
(`GraphDesc::upgrade`: the wire moves to `@time`, `@bits` or `@cutoff`, and the depth becomes its amount).
