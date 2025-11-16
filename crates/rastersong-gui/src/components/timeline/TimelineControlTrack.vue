<script setup>
import { computed, ref, onMounted, onUnmounted } from 'vue'

const props = defineProps({
  pixelsPerSecond: {
    type: Number,
    required: true,
    default: 20
  },
  viewportWidth: {
    type: Number,
    default: 0
  },
  timelineStart: {
    type: Number,
    required: true,
    default: 0
  },
  timelineEnd: {
    type: Number,
    required: true,
    default: 60
  },
  playheadPosition: {
    type: Number,
    required: true,
    default: 0
  },
  selectionStart: {
    type: Number,
    default: null
  },
  selectionEnd: {
    type: Number,
    default: null
  }
})

const emit = defineEmits(['update:playhead', 'update:selection'])

const controlTrackRef = ref(null)
const isDraggingPlayhead = ref(false)
const isSelecting = ref(false)
const isDraggingStartMarker = ref(false)
const isDraggingEndMarker = ref(false)

// Track mouse down state to distinguish clicks from drags
const mouseDownState = ref({
  isDown: false,
  startX: 0,
  startTime: 0,
  hasMoved: false
})

// Track last mouse position for mouseup handling
const lastMousePosition = ref({
  x: 0,
  time: 0
})

const DRAG_THRESHOLD = 3 // pixels - minimum movement to be considered a drag

const timeMarkers = computed(() => {
  const markers = []

  const pixelsPerSecond = props.pixelsPerSecond || 1
  const visibleStart = props.timelineStart
  const visibleEnd = props.timelineEnd

  // Determine marker interval based on zoom level
  let interval = 5
  if (pixelsPerSecond < 10) {
    interval = 30
  } else if (pixelsPerSecond < 20) {
    interval = 10
  } else if (pixelsPerSecond < 50) {
    interval = 5
  } else if (pixelsPerSecond < 100) {
    interval = 2
  } else {
    interval = 1
  }

  // Find first marker that should be visible
  const firstMarker = Math.floor(visibleStart / interval) * interval

  // Generate markers only for visible range
  for (let time = firstMarker; time <= visibleEnd; time += interval) {
    markers.push(time)
  }

  return markers
})

const hasSelection = computed(() => {
  return props.selectionStart !== null && 
         props.selectionEnd !== null && 
         props.selectionStart !== props.selectionEnd
})

// Convert time to pixel position within the viewport
const timeToPixel = (time) => {
  return (time - props.timelineStart) * props.pixelsPerSecond
}

const playheadStyle = computed(() => {
  return {
    left: timeToPixel(props.playheadPosition) + 'px'
  }
})

const selectionStyle = computed(() => {
  if (!hasSelection.value) return { display: 'none' }
  const start = Math.min(props.selectionStart, props.selectionEnd)
  const end = Math.max(props.selectionStart, props.selectionEnd)
  return {
    left: timeToPixel(start) + 'px',
    width: ((end - start) * props.pixelsPerSecond) + 'px'
  }
})

const getSelectionMarkerStyle = (isStart) => {
  if (!hasSelection.value) return { display: 'none' }
  const time = isStart 
    ? Math.min(props.selectionStart, props.selectionEnd) 
    : Math.max(props.selectionStart, props.selectionEnd)
  return {
    left: timeToPixel(time) + 'px'
  }
}

const formatTime = (seconds) => {
  const mins = Math.floor(seconds / 60)
  const secs = seconds % 60
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

const pixelToTime = (pixelX) => {
  // Convert pixel position to time, accounting for viewport offset
  const time = props.timelineStart + (pixelX / props.pixelsPerSecond)
  return Math.max(0, time)
}

const handleControlTrackMouseDown = (event) => {
  // Only respond to left mouse button
  if (event.button !== 0) return
  
  if (!controlTrackRef.value) return
  const rect = controlTrackRef.value.getBoundingClientRect()
  const x = event.clientX - rect.left
  const time = pixelToTime(x)
  
  // Record mouse down state
  mouseDownState.value = {
    isDown: true,
    startX: x,
    startTime: time,
    hasMoved: false
  }
  
  // Initialize last mouse position
  lastMousePosition.value = { x, time }
  
  // Check if clicking near playhead (for potential drag)
  const playheadX = timeToPixel(props.playheadPosition)
  if (Math.abs(x - playheadX) < 8) {
    isDraggingPlayhead.value = true
  }
  
  event.preventDefault()
}

const handleSelectionMarkerMouseDown = (isStart, event) => {
  // Only respond to left mouse button
  if (event.button !== 0) return
  
  if (isStart) {
    isDraggingStartMarker.value = true
  } else {
    isDraggingEndMarker.value = true
  }
  
  event.stopPropagation()
}

const handleGlobalMouseMove = (event) => {
  if (!controlTrackRef.value) return
  const rect = controlTrackRef.value.getBoundingClientRect()
  const x = event.clientX - rect.left
  const time = pixelToTime(x)
  
  // Track last mouse position for mouseup handling
  lastMousePosition.value = { x, time }
  
  // Check if mouse has moved beyond threshold to start dragging
  if (mouseDownState.value.isDown && !mouseDownState.value.hasMoved) {
    const deltaX = Math.abs(x - mouseDownState.value.startX)
    if (deltaX >= DRAG_THRESHOLD) {
      mouseDownState.value.hasMoved = true
      
      // Start selection drag if not dragging playhead or markers
      if (!isDraggingPlayhead.value && !isDraggingStartMarker.value && !isDraggingEndMarker.value) {
        isSelecting.value = true
        // Set selection START to the mouse down position
        emit('update:selection', {
          start: mouseDownState.value.startTime,
          end: mouseDownState.value.startTime // Initially same as start
        })
      }
    }
  }
  
  // Handle active drags
  if (isDraggingStartMarker.value || isDraggingEndMarker.value) {
    if (isDraggingStartMarker.value) {
      emit('update:selection', {
        start: time,
        end: props.selectionEnd
      })
    } else if (isDraggingEndMarker.value) {
      emit('update:selection', {
        start: props.selectionStart,
        end: time
      })
    }
  } else if (isDraggingPlayhead.value) {
    emit('update:playhead', time)
  } else if (isSelecting.value) {
    // During selection drag, update the END position
    emit('update:selection', {
      start: mouseDownState.value.startTime, // Keep start fixed
      end: time // Update end as we drag
    })
  }
}

const handleGlobalMouseUp = (event) => {
  let time = lastMousePosition.value.time
  
  // Try to get current position from event if control track is available
  if (controlTrackRef.value) {
    const rect = controlTrackRef.value.getBoundingClientRect()
    const x = event.clientX - rect.left
    // Only use event position if it's within bounds
    if (x >= 0 && x <= rect.width) {
      time = pixelToTime(x)
      lastMousePosition.value = { x, time }
    }
  }
  
  // If it was a selection drag, set the END position on mouse up
  if (isSelecting.value) {
    emit('update:selection', {
      start: mouseDownState.value.startTime,
      end: time // Final end position
    })
  }
  // If mouse was down but didn't move (click, not drag), move playhead
  else if (mouseDownState.value.isDown && !mouseDownState.value.hasMoved && 
      !isDraggingPlayhead.value && !isDraggingStartMarker.value && !isDraggingEndMarker.value) {
    emit('update:playhead', mouseDownState.value.startTime)
  }
  
  // Reset all drag states
  isDraggingStartMarker.value = false
  isDraggingEndMarker.value = false
  isDraggingPlayhead.value = false
  isSelecting.value = false
  mouseDownState.value.isDown = false
  mouseDownState.value.hasMoved = false
}

// Removed: handleControlTrackClick - clicks are now handled in handleGlobalMouseUp

onMounted(() => {
  document.addEventListener('mousemove', handleGlobalMouseMove)
  document.addEventListener('mouseup', handleGlobalMouseUp)
})

onUnmounted(() => {
  document.removeEventListener('mousemove', handleGlobalMouseMove)
  document.removeEventListener('mouseup', handleGlobalMouseUp)
})
</script>

<template>
  <div class="control-track-wrapper">
    <div class="control-track-label"></div>
    <div
      ref="controlTrackRef"
      class="control-track"
      @mousedown="handleControlTrackMouseDown">
      <div
        v-if="hasSelection"
        class="selection-region"
        :style="selectionStyle">
      </div>
    <div
      v-for="marker in timeMarkers"
      :key="marker"
      class="time-marker"
      :style="{ left: timeToPixel(marker) + 'px' }">
        <div class="marker-line"></div>
        <div class="marker-label">{{ formatTime(marker) }}</div>
      </div>
      <div
        v-if="hasSelection"
        class="selection-marker start-marker"
        :style="getSelectionMarkerStyle(true)"
        @mousedown="handleSelectionMarkerMouseDown(true, $event)">
      </div>
      <div
        v-if="hasSelection"
        class="selection-marker end-marker"
        :style="getSelectionMarkerStyle(false)"
        @mousedown="handleSelectionMarkerMouseDown(false, $event)">
      </div>
      <div
        class="playhead"
        :style="playheadStyle"
        :class="{ dragging: isDraggingPlayhead }">
      </div>
    </div>
  </div>
</template>

<style scoped>
.control-track-wrapper {
  display: flex;
  flex-shrink: 0;
  width: 100%;
}

.control-track-label {
  width: 80px;
  background-color: #1a1a1a;
  border-right: 1px solid #2a2a2a;
  border-bottom: 2px solid #3a3a3a;
  flex-shrink: 0;
  z-index: 10;
  position: relative;
}

.control-track {
  min-height: 30px;
  height: 30px;
  background-color: #1e1e1e;
  border-bottom: 2px solid #3a3a3a;
  position: relative;
  cursor: crosshair;
  user-select: none;
  flex: 1;
  z-index: 5;
  overflow: hidden;
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
  top: 50%;
  left: 2px;
  transform: translateY(-50%);
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
  transform: translateX(-50%);
}

.playhead::before {
  content: '';
  position: absolute;
  top: 0;
  left: 50%;
  transform: translateX(-50%);
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
  width: 2px;
}

.control-track .playhead:hover {
  width: 6px;
  background-color: #ff6666;
}

.control-track .playhead.dragging {
  width: 4px;
}
</style>

