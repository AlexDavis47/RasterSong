<script setup>
import { ref, onMounted, onUnmounted } from 'vue'

// Sample audio clips data
const audioClips = [
  { id: 1, track: 0, start: 0, duration: 5, name: 'Video Carrier', isCarrier: true },
  { id: 3, track: 1, start: 2, duration: 6, name: 'Modulator 1', isCarrier: false },
  { id: 4, track: 2, start: 8, duration: 3, name: 'Modulator 2', isCarrier: false },
]

const carrierTrackId = 0
const modulatorTracks = [1, 2, 3]

const timeMarkers = []
const totalTime = 30 // seconds
const pixelsPerSecond = 20

for (let i = 0; i <= totalTime; i += 5) {
  timeMarkers.push(i)
}

const playheadPosition = ref(5) // seconds
const selectionStart = ref(null)
const selectionEnd = ref(null)

// Interaction state
const isDraggingPlayhead = ref(false)
const isDraggingSelection = ref(false)
const isSelecting = ref(false)
const isDraggingStartMarker = ref(false)
const isDraggingEndMarker = ref(false)
const dragStartX = ref(0)

const formatTime = (seconds) => {
  const mins = Math.floor(seconds / 60)
  const secs = seconds % 60
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

const hasSelection = () => {
  return selectionStart.value !== null && selectionEnd.value !== null && selectionStart.value !== selectionEnd.value
}

// For demo purposes, set a sample selection
selectionStart.value = 2
selectionEnd.value = 8

const getSelectionStyle = () => {
  if (!hasSelection()) return { display: 'none' }
  const start = Math.min(selectionStart.value, selectionEnd.value)
  const end = Math.max(selectionStart.value, selectionEnd.value)
  return {
    left: (start * pixelsPerSecond) + 'px',
    width: ((end - start) * pixelsPerSecond) + 'px'
  }
}

const getPlayheadStyle = () => {
  return {
    left: (playheadPosition.value * pixelsPerSecond) + 'px'
  }
}

const getSelectionMarkerStyle = (isStart) => {
  const time = isStart ? Math.min(selectionStart.value, selectionEnd.value) : Math.max(selectionStart.value, selectionEnd.value)
  return {
    left: (time * pixelsPerSecond) + 'px'
  }
}

const pixelToTime = (pixelX) => {
  return Math.max(0, Math.min(totalTime, pixelX / pixelsPerSecond))
}

const handleControlTrackMouseDown = (event) => {
  const rect = event.currentTarget.getBoundingClientRect()
  const x = event.clientX - rect.left
  const time = pixelToTime(x)
  
  // Check if clicking near playhead
  const playheadX = playheadPosition.value * pixelsPerSecond
  if (Math.abs(x - playheadX) < 8) {
    isDraggingPlayhead.value = true
    dragStartX.value = x
  } else {
    // Start selection
    isSelecting.value = true
    selectionStart.value = time
    selectionEnd.value = time
    dragStartX.value = x
  }
  
  event.preventDefault()
}

const handleControlTrackMouseMove = () => {
  // Handled by global handler
}

const handleControlTrackMouseUp = () => {
  handleGlobalMouseUp()
}

const handleControlTrackClick = (event) => {
  // Only move playhead if we didn't just finish a drag
  if (!isDraggingPlayhead.value && !isSelecting.value && !isDraggingStartMarker.value && !isDraggingEndMarker.value) {
    const rect = event.currentTarget.getBoundingClientRect()
    const x = event.clientX - rect.left
    playheadPosition.value = pixelToTime(x)
  }
}

const handleSelectionMarkerMouseDown = (isStart, event) => {
  if (isStart) {
    isDraggingStartMarker.value = true
  } else {
    isDraggingEndMarker.value = true
  }
  event.stopPropagation()
}

const controlTrackRef = ref(null)

const handleGlobalMouseMove = (event) => {
  if (isDraggingStartMarker.value || isDraggingEndMarker.value) {
    if (!controlTrackRef.value) return
    const rect = controlTrackRef.value.getBoundingClientRect()
    const x = event.clientX - rect.left
    const time = pixelToTime(x)
    
    if (isDraggingStartMarker.value) {
      selectionStart.value = time
    } else if (isDraggingEndMarker.value) {
      selectionEnd.value = time
    }
  } else if (isDraggingPlayhead.value || isSelecting.value) {
    if (!controlTrackRef.value) return
    const rect = controlTrackRef.value.getBoundingClientRect()
    const x = event.clientX - rect.left
    const time = pixelToTime(x)
    
    if (isDraggingPlayhead.value) {
      playheadPosition.value = time
    } else if (isSelecting.value) {
      selectionEnd.value = time
    }
  }
}

const handleGlobalMouseUp = () => {
  isDraggingStartMarker.value = false
  isDraggingEndMarker.value = false
  isDraggingPlayhead.value = false
  isSelecting.value = false
}

onMounted(() => {
  document.addEventListener('mousemove', handleGlobalMouseMove)
  document.addEventListener('mouseup', handleGlobalMouseUp)
})

onUnmounted(() => {
  document.removeEventListener('mousemove', handleGlobalMouseMove)
  document.removeEventListener('mouseup', handleGlobalMouseUp)
})

const handleSelectionMarkerMouseMove = () => {
  // Handled by global handler
}

const handleSelectionMarkerMouseUp = () => {
  handleGlobalMouseUp()
}
</script>

<template>
  <div class="timeline-panel panel">
    <div class="panel-header">Timeline</div>
    <div class="timeline-content">
      <div 
        ref="controlTrackRef"
        class="control-track"
        @mousedown="handleControlTrackMouseDown"
        @mousemove="handleControlTrackMouseMove"
        @mouseup="handleControlTrackMouseUp"
        @click="handleControlTrackClick"
        @mouseleave="handleControlTrackMouseUp">
        <div 
          v-if="hasSelection()"
          class="selection-region"
          :style="getSelectionStyle()">
        </div>
        <div 
          v-for="marker in timeMarkers" 
          :key="marker"
          class="time-marker"
          :style="{ left: (marker * pixelsPerSecond) + 'px' }">
          <div class="marker-line"></div>
          <div class="marker-label">{{ formatTime(marker) }}</div>
        </div>
        <div 
          v-if="hasSelection()"
          class="selection-marker start-marker"
          :style="getSelectionMarkerStyle(true)"
          @mousedown="handleSelectionMarkerMouseDown(true, $event)">
        </div>
        <div 
          v-if="hasSelection()"
          class="selection-marker end-marker"
          :style="getSelectionMarkerStyle(false)"
          @mousedown="handleSelectionMarkerMouseDown(false, $event)">
        </div>
        <div 
          class="playhead"
          :style="getPlayheadStyle()"
          :class="{ dragging: isDraggingPlayhead }">
        </div>
      </div>
      <div class="timeline-tracks">
        <div class="track carrier-track">
          <div class="track-label">Video Carrier</div>
          <div class="track-content">
            <div 
              v-for="clip in audioClips.filter(c => c.track === carrierTrackId)"
              :key="clip.id"
              class="audio-clip carrier-clip"
              :style="{ 
                left: (clip.start * pixelsPerSecond) + 'px',
                width: (clip.duration * pixelsPerSecond) + 'px'
              }">
              {{ clip.name }}
            </div>
          </div>
        </div>
        <div class="track-separator"></div>
        <div 
          v-for="track in modulatorTracks" 
          :key="track"
          class="track">
          <div class="track-label">Modulator {{ track }}</div>
          <div class="track-content">
            <div 
              v-for="clip in audioClips.filter(c => c.track === track)"
              :key="clip.id"
              class="audio-clip"
              :style="{ 
                left: (clip.start * pixelsPerSecond) + 'px',
                width: (clip.duration * pixelsPerSecond) + 'px'
              }">
              {{ clip.name }}
            </div>
          </div>
        </div>
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

.timeline-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-x: auto;
  overflow-y: hidden;
}

.control-track {
  min-height: 30px;
  height: 30px;
  background-color: #1e1e1e;
  border-bottom: 2px solid #3a3a3a;
  position: relative;
  min-width: 600px;
  cursor: crosshair;
  user-select: none;
  flex-shrink: 0;
  z-index: 5;
}

.control-track:active {
  cursor: grabbing;
}

.selection-region {
  position: absolute;
  top: 0;
  height: 100%;
  background-color: rgba(74, 144, 226, 0.3);
  border-left: 2px solid #4a90e2;
  border-right: 2px solid #4a90e2;
  pointer-events: none;
  z-index: 5;
}

.selection-marker {
  position: absolute;
  top: 0;
  width: 4px;
  height: 100%;
  background-color: #4a90e2;
  cursor: ew-resize;
  z-index: 15;
  pointer-events: auto;
}

.selection-marker:hover {
  background-color: #5aa0f2;
  width: 6px;
}

.selection-marker.start-marker {
  border-left: 2px solid #fff;
}

.selection-marker.end-marker {
  border-right: 2px solid #fff;
}

.time-marker {
  position: absolute;
  top: 0;
  height: 100%;
}

.marker-line {
  width: 1px;
  height: 20px;
  background-color: #444;
  margin-top: 5px;
}

.marker-label {
  position: absolute;
  top: 22px;
  left: 2px;
  font-size: 10px;
  color: #888;
  white-space: nowrap;
}

.playhead {
  position: absolute;
  top: 0;
  width: 2px;
  height: 100%;
  background-color: #ff4444;
  z-index: 10;
  pointer-events: none;
  cursor: ew-resize;
}

.playhead::before {
  content: '';
  position: absolute;
  top: 0;
  left: -4px;
  width: 0;
  height: 0;
  border-left: 5px solid transparent;
  border-right: 5px solid transparent;
  border-top: 6px solid #ff4444;
}

.playhead.dragging {
  cursor: grabbing;
}

.control-track .playhead {
  pointer-events: auto;
  cursor: ew-resize;
  width: 4px;
}

.control-track .playhead:hover {
  width: 6px;
  background-color: #ff6666;
}

.timeline-tracks {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 600px;
  overflow-y: auto;
}

.track-separator {
  height: 3px;
  background-color: #3a3a3a;
  border-top: 1px solid #2a2a2a;
  border-bottom: 1px solid #2a2a2a;
  flex-shrink: 0;
}

.track {
  display: flex;
  height: 50px;
  border-bottom: 1px solid #2a2a2a;
  flex-shrink: 0;
}

.carrier-track {
  background-color: #1a1a1e;
}

.track-label {
  width: 80px;
  padding: 8px;
  background-color: #1a1a1a;
  border-right: 1px solid #2a2a2a;
  font-size: 11px;
  color: #888;
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.track-content {
  flex: 1;
  position: relative;
  background-color: #1e1e1e;
}

.audio-clip {
  position: absolute;
  top: 4px;
  height: calc(100% - 8px);
  background: linear-gradient(135deg, #4a90e2 0%, #357abd 100%);
  border: 1px solid #5aa0f2;
  border-radius: 3px;
  padding: 4px 8px;
  font-size: 11px;
  color: #fff;
  display: flex;
  align-items: center;
  cursor: pointer;
  transition: all 0.2s;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.audio-clip:hover {
  background: linear-gradient(135deg, #5aa0f2 0%, #4580cd 100%);
  border-color: #6ab0ff;
  box-shadow: 0 2px 8px rgba(74, 144, 226, 0.3);
}

.carrier-clip {
  background: linear-gradient(135deg, #e24a4a 0%, #bd3535 100%);
  border: 1px solid #f25a5a;
}

.carrier-clip:hover {
  background: linear-gradient(135deg, #f25a5a 0%, #cd4545 100%);
  border-color: #ff6a6a;
  box-shadow: 0 2px 8px rgba(226, 74, 74, 0.3);
}
</style>

