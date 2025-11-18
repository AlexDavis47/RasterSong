<script setup lang="ts">
import { ref } from 'vue'
import MenuBar from './components/MenuBar.vue'
import PreviewPanel from './components/PreviewPanel.vue'
import TimelinePanel from './components/timeline/TimelinePanel.vue'
import NodeGraphPanel from './components/NodeGraphPanel.vue'
import PropertiesPanel from './components/PropertiesPanel.vue'

// Track playhead position globally (start at 5s to match TimelinePanel)
const playheadPosition = ref(5)

const handlePlayheadUpdate = (time: number) => {
  console.log('App received playhead update:', time)
  playheadPosition.value = time
}
</script>

<template>
  <div class="app-container">
    <MenuBar />
    <PreviewPanel :playhead-position="playheadPosition" />
    <div class="node-graph-wrapper">
      <NodeGraphPanel />
      <PropertiesPanel />
    </div>
    <TimelinePanel @update:playhead="handlePlayheadUpdate" />
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
