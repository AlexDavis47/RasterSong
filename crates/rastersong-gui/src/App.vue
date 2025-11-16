<script setup>
import MenuBar from './components/MenuBar.vue'
import PreviewPanel from './components/PreviewPanel.vue'
import TimelinePanel from './components/timeline/TimelinePanel.vue'
import NodeGraphPanel from './components/NodeGraphPanel.vue'
import PropertiesPanel from './components/PropertiesPanel.vue'
</script>

<template>
  <div class="app-container">
    <MenuBar />
    <PreviewPanel />
    <div class="node-graph-wrapper">
      <NodeGraphPanel />
      <PropertiesPanel />
    </div>
    <TimelinePanel />
  </div>
</template>

<style scoped>
.app-container {
  width: 100vw;
  height: 100vh;
  display: grid;
  grid-template-rows: 50px 1fr 300px;
  grid-template-columns: 40% 60%;
  grid-template-areas:
    "menu menu"
    "preview nodegraph"
    "timeline timeline";
  overflow: hidden;
}

.node-graph-wrapper {
  grid-area: nodegraph;
  position: relative;
  overflow: hidden;
}
</style>
