//! Macros that keep a node's definition in its own file: `params!` declares parameters with named
//! indices, and `choice!` declares an enum for a choice parameter.

/// Declares a node's parameters and an index constant for each.
///
/// ```ignore
/// params! { Distortion {
///     SHAPE: ParamSpec::choice("shape", ...),
///     DRIVE: ParamSpec::number("drive", ...),
/// } }
/// ```
///
/// Expands to `Distortion::PARAMS` (the specs, in order) and `Distortion::SHAPE`, `Distortion::DRIVE`
/// (each parameter's index, for [`Params`](crate::Params) and
/// [`ProcessContext::value`](crate::ProcessContext::value)). The names must match: a constant
/// `DRIVE` must be the parameter `"drive"`, which a compile-time assertion checks, so reordering
/// or renaming parameters can't silently change which one a node reads.
macro_rules! params {
    ($ty:ident { $($index:ident : $spec:expr),* $(,)? }) => {
        impl $ty {
            pub const PARAMS: &'static [$crate::ParamSpec] = &[$($spec),*];
            params!(@index 0usize; $($index)*);
        }
        const _: () = {
            $(assert!(
                $crate::nodes::name_matches($ty::PARAMS[$ty::$index].name, stringify!($index)),
                concat!("the parameter named by `", stringify!($index), "` is not in that position"),
            );)*
        };
    };
    (@index $n:expr;) => {};
    (@index $n:expr; $head:ident $($tail:ident)*) => {
        pub const $head: usize = $n;
        params!(@index $n + 1usize; $($tail)*);
    };
}

/// Declares an enum for a choice parameter. Each variant names the option it stands for.
///
/// ```ignore
/// choice! {
///     /// How the signal is bent.
///     pub enum Shape { Soft = "soft", Hard = "hard" }
/// }
/// ```
///
/// Gives the enum `OPTIONS` (for `ParamSpec::choice`) and [`Choice`](crate::nodes::Choice), so
/// [`Params::choice_as`](crate::Params::choice_as) reads it with no fallback arm.
macro_rules! choice {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $option:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// The options a parameter of this type offers, in order. The first is the default
            /// unless the parameter says otherwise.
            pub const OPTIONS: &'static [&'static str] = &[$($option),+];
        }

        impl $crate::nodes::Choice for $name {
            fn from_option(option: &str) -> Option<Self> {
                match option {
                    $($option => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}
