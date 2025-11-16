<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import TimelineTrack from './TimelineTrack.vue'
import TimelineControlTrack from './TimelineControlTrack.vue'

const pixelsPerSecond = ref(20)
const minZoom = 5
const maxZoom = 200
const zoomStep = 5

const playheadPosition = ref(5) // seconds
const selectionStart = ref(null)
const selectionEnd = ref(null)
const timelineContentRef = ref(null)
const timelineViewportWidth = ref(0)

// Timeline viewport: tracks what time range we're currently viewing
const timelineStart = ref(0) // Start time in seconds
const timelineEnd = ref(60) // End time in seconds (will be calculated based on viewport)

// Calculate timeline end based on viewport width and scale
const updateTimelineEnd = () => {
  if (timelineViewportWidth.value > 0) {
    const viewportDuration = timelineViewportWidth.value / pixelsPerSecond.value
    timelineEnd.value = timelineStart.value + viewportDuration
  }
}

const zoomIn = () => {
  const oldPPS = pixelsPerSecond.value
  pixelsPerSecond.value = Math.min(maxZoom, pixelsPerSecond.value + zoomStep)
  updateTimelineEnd()
}

const zoomOut = () => {
  const oldPPS = pixelsPerSecond.value
  pixelsPerSecond.value = Math.max(minZoom, pixelsPerSecond.value - zoomStep)
  updateTimelineEnd()
}

// Panning state
const isPanning = ref(false)
const panStartX = ref(0)
const panStartTime = ref(0)

const handleWheel = (event) => {
  // Only zoom if Ctrl/Cmd key is held (standard zoom behavior)
  if (event.ctrlKey || event.metaKey) {
    event.preventDefault()
    if (event.deltaY < 0) {
      zoomIn()
    } else {
      zoomOut()
    }
  } else {
    // Pan horizontally with regular scroll
    event.preventDefault()
    const pixelDelta = event.deltaY
    const timeDelta = pixelDelta / pixelsPerSecond.value
    
    // Prevent panning before time 0
    const newStart = Math.max(0, timelineStart.value + timeDelta)
    const duration = timelineEnd.value - timelineStart.value
    timelineStart.value = newStart
    timelineEnd.value = newStart + duration
  }
}

const handleMouseDown = (event) => {
  // Middle mouse button or space+left button for panning
  if (event.button === 1 || (event.button === 0 && event.shiftKey)) {
    event.preventDefault()
    isPanning.value = true
    panStartX.value = event.clientX
    panStartTime.value = timelineStart.value
  }
}

const handleMouseMove = (event) => {
  if (isPanning.value) {
    const pixelDelta = panStartX.value - event.clientX
    const timeDelta = pixelDelta / pixelsPerSecond.value
    
    // Prevent panning before time 0
    const newStart = Math.max(0, panStartTime.value + timeDelta)
    const duration = timelineEnd.value - timelineStart.value
    timelineStart.value = newStart
    timelineEnd.value = newStart + duration
  }
}

const handleMouseUp = () => {
  isPanning.value = false
}

// Video tracks - contains carrier clips
const videoTracks = ref([
  {
    id: 0,
    label: 'Video Carrier',
    isCarrier: true,
    clips: [
      { id: 1, name: 'Video Carrier', start: 0, duration: 5, isCarrier: true }
    ]
  }
])

// Audio tracks - contains modulator clips
const audioTracks = ref([
  {
    id: 1,
    label: 'Modulator 1',
    isCarrier: false,
    clips: [
      { id: 3, name: 'Modulator 1', start: 2, duration: 6, isCarrier: false }
    ]
  },
  {
    id: 2,
    label: 'Modulator 2',
    isCarrier: false,
    clips: [
      { id: 4, name: 'Modulator 2', start: 8, duration: 3, isCarrier: false }
    ]
  }
])

const updateViewportWidth = () => {
  if (timelineContentRef.value) {
    timelineViewportWidth.value = timelineContentRef.value.clientWidth
    updateTimelineEnd()
  }
}

let resizeObserver = null

onMounted(() => {
  updateViewportWidth()
  if (timelineContentRef.value && window.ResizeObserver) {
    resizeObserver = new ResizeObserver(() => {
      updateViewportWidth()
    })
    resizeObserver.observe(timelineContentRef.value)
  } else {
    window.addEventListener('resize', updateViewportWidth)
  }
  
  // Add global mouse event listeners for panning
  document.addEventListener('mousemove', handleMouseMove)
  document.addEventListener('mouseup', handleMouseUp)
})

onUnmounted(() => {
  if (resizeObserver) {
    resizeObserver.disconnect()
  } else {
    window.removeEventListener('resize', updateViewportWidth)
  }
  
  // Remove global mouse event listeners
  document.removeEventListener('mousemove', handleMouseMove)
  document.removeEventListener('mouseup', handleMouseUp)
})

const handleClipStartUpdate = (data) => {
  const { trackId, clipId, newStartTime } = data
  
  // Search in video tracks
  for (const track of videoTracks.value) {
    if (track.id === trackId) {
      const clip = track.clips.find(c => c.id === clipId)
      if (clip) {
        clip.start = newStartTime
        return
      }
    }
  }
  
  // Search in audio tracks
  for (const track of audioTracks.value) {
    if (track.id === trackId) {
      const clip = track.clips.find(c => c.id === clipId)
      if (clip) {
        clip.start = newStartTime
        return
      }
    }
  }
}

const handlePlayheadUpdate = (time) => {
  playheadPosition.value = time
}

const handleSelectionUpdate = (data) => {
  selectionStart.value = data.start
  selectionEnd.value = data.end
}
</script>

<template>
  <div class="timeline-panel panel">
    <div class="panel-header">
      <span>Timeline</span>
      <div class="zoom-controls">
        <button class="zoom-button" @click="zoomOut" title="Zoom Out">−</button>
        <span class="zoom-level">{{ Math.round(pixelsPerSecond) }}px/s</span>
        <button class="zoom-button" @click="zoomIn" title="Zoom In">+</button>
      </div>
    </div>
    <div 
      ref="timelineContentRef"
      class="timeline-content" 
      @wheel="handleWheel"
      @mousedown="handleMouseDown">
      <TimelineControlTrack
        :pixels-per-second="pixelsPerSecond"
        :viewport-width="timelineViewportWidth"
        :timeline-start="timelineStart"
        :timeline-end="timelineEnd"
        :playhead-position="playheadPosition"
        :selection-start="selectionStart"
        :selection-end="selectionEnd"
        @update:playhead="handlePlayheadUpdate"
        @update:selection="handleSelectionUpdate"
      />
      <div class="timeline-tracks">
        <TimelineTrack
          v-for="track in videoTracks"
          :key="track.id"
          :track="track"
          :pixels-per-second="pixelsPerSecond"
          :viewport-width="timelineViewportWidth"
          :timeline-start="timelineStart"
          :timeline-end="timelineEnd"
          @update:clip-start="handleClipStartUpdate"
        />
        <div class="track-separator"></div>
        <TimelineTrack
          v-for="track in audioTracks"
          :key="track.id"
          :track="track"
          :pixels-per-second="pixelsPerSecond"
          :viewport-width="timelineViewportWidth"
          :timeline-start="timelineStart"
          :timeline-end="timelineEnd"
          @update:clip-start="handleClipStartUpdate"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.timeline-panel {
  grid-area: timeline;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.zoom-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}

.zoom-button {
  background-color: #2a2a2a;
  border: 1px solid #3a3a3a;
  color: #ccc;
  width: 24px;
  height: 24px;
  border-radius: 3px;
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.zoom-button:hover {
  background-color: #333;
  border-color: #4a4a4a;
  color: #fff;
}

.zoom-button:active {
  background-color: #1a1a1a;
}

.zoom-level {
  font-size: 11px;
  color: #888;
  min-width: 50px;
  text-align: center;
  font-weight: normal;
  text-transform: none;
}

.timeline-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
  width: 100%;
  cursor: grab;
}

.timeline-content:active {
  cursor: grabbing;
}

.timeline-tracks {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  width: 100%;
}

.track-separator {
  height: 3px;
  background-color: #3a3a3a;
  border-top: 1px solid #2a2a2a;
  border-bottom: 1px solid #2a2a2a;
  flex-shrink: 0;
}
</style>

