<script setup>
// Pre-positioned nodes
const nodes = [
  { id: 'carrier', type: 'input', label: 'Carrier', x: 50, y: 100, outputs: ['out1'] },
  { id: 'modulator1', type: 'input', label: 'Modulator 1', x: 50, y: 200, outputs: ['out1'] },
  { id: 'modulator2', type: 'input', label: 'Modulator 2', x: 50, y: 300, outputs: ['out1'] },
  { id: 'am', type: 'process', label: 'Amplitude Modulation', x: 300, y: 200, inputs: ['in1', 'in2'], outputs: ['out1'] },
  { id: 'output', type: 'output', label: 'Output', x: 550, y: 200, inputs: ['in1'] },
]

// Connections between nodes
const connections = [
  { from: 'carrier', fromOutput: 'out1', to: 'am', toInput: 'in1' },
  { id: 'mod1-conn', from: 'modulator1', fromOutput: 'out1', to: 'am', toInput: 'in2' },
  { from: 'am', fromOutput: 'out1', to: 'output', toInput: 'in1' },
]

const getNodeStyle = (node) => {
  return {
    left: node.x + 'px',
    top: node.y + 'px',
  }
}

const getConnectionPath = (fromNode, toNode) => {
  const fromX = fromNode.x + 120
  const fromY = fromNode.y + 30
  const toX = toNode.x
  const toY = toNode.y + 30
  
  const midX = (fromX + toX) / 2
  
  return `M ${fromX} ${fromY} C ${midX} ${fromY}, ${midX} ${toY}, ${toX} ${toY}`
}
</script>

<template>
  <div class="node-graph-panel panel">
    <div class="panel-header">Node Graph</div>
    <div class="node-graph-content">
      <svg class="connections-layer" width="100%" height="100%">
        <defs>
          <marker id="arrowhead" markerWidth="10" markerHeight="10" refX="9" refY="3" orient="auto">
            <polygon points="0 0, 10 3, 0 6" fill="#4a90e2" />
          </marker>
        </defs>
        <path
          v-for="(conn, index) in connections"
          :key="index"
          :d="getConnectionPath(
            nodes.find(n => n.id === conn.from),
            nodes.find(n => n.id === conn.to)
          )"
          class="connection-line"
          stroke="#4a90e2"
          stroke-width="2"
          fill="none"
          marker-end="url(#arrowhead)"
        />
      </svg>
      <div class="nodes-layer">
        <div
          v-for="node in nodes"
          :key="node.id"
          class="node"
          :class="node.type"
          :style="getNodeStyle(node)">
          <div class="node-header">{{ node.label }}</div>
          <div class="node-body">
            <div v-if="node.inputs" class="node-inputs">
              <div
                v-for="input in node.inputs"
                :key="input"
                class="connector input-connector"
                :data-input="input">
              </div>
            </div>
            <div class="node-content">
              <div v-if="node.type === 'process'" class="node-icon">AM</div>
            </div>
            <div v-if="node.outputs" class="node-outputs">
              <div
                v-for="output in node.outputs"
                :key="output"
                class="connector output-connector"
                :data-output="output">
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.node-graph-panel {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.node-graph-content {
  flex: 1;
  position: relative;
  min-width: 700px;
  min-height: 400px;
  background: linear-gradient(90deg, #1a1a1a 0%, #1e1e1e 100%);
  background-size: 20px 20px;
  background-image: 
    linear-gradient(to right, #2a2a2a 1px, transparent 1px),
    linear-gradient(to bottom, #2a2a2a 1px, transparent 1px);
  overflow: auto;
}

.connections-layer {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 1;
}

.connection-line {
  pointer-events: stroke;
}

.nodes-layer {
  position: relative;
  width: 100%;
  height: 100%;
  z-index: 2;
}

.node {
  position: absolute;
  width: 120px;
  min-height: 60px;
  background-color: #2a2a2a;
  border: 2px solid #3a3a3a;
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  cursor: default;
}

.node.input {
  border-color: #4a90e2;
}

.node.process {
  border-color: #e2a04a;
  width: 180px;
}

.node.output {
  border-color: #4ae2a0;
}

.node-header {
  padding: 8px 12px;
  background-color: #333;
  border-bottom: 1px solid #3a3a3a;
  font-size: 11px;
  font-weight: 600;
  color: #fff;
  text-align: center;
  border-radius: 4px 4px 0 0;
}

.node-body {
  padding: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
}

.node-inputs {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.node-outputs {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-left: auto;
}

.node-content {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.node-icon {
  background-color: #3a3a3a;
  padding: 6px 12px;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 600;
  color: #e2a04a;
}

.connector {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: 2px solid #666;
  background-color: #2a2a2a;
  cursor: pointer;
  transition: all 0.2s;
}

.input-connector {
  margin-left: -16px;
}

.output-connector {
  margin-right: -16px;
}

.connector:hover {
  border-color: #4a90e2;
  background-color: #4a90e2;
  transform: scale(1.2);
}
</style>
