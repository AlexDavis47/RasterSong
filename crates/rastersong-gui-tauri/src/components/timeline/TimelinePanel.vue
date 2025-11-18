<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue'
import TimelineTrack from './TimelineTrack.vue'
import TimelineControlTrack from './TimelineControlTrack.vue'
import { useTimelineStore } from '../../composables/useTimelineStore'
import { removeMultipleMedia } from '../../utils/media'

const props = defineProps<{
  playheadPosition: number
}>()

const { videoTracks, audioTracks, removeVideoTrack, removeAudioTrack } = useTimelineStore()

const emit = defineEmits<{
  'update:playhead': [time: number]
}>()

const pixelsPerSecond = ref(20)
const minZoom = 0.1
const maxZoom = 1000
const zoomStep = 1

// Local playhead position for internal use (synced with prop)
const playheadPosition = ref(props.playheadPosition)

// Sync local playhead with prop (for external updates like playback)
watch(() => props.playheadPosition, (newPosition) => {
  // Only update if different to avoid unnecessary updates
  if (Math.abs(playheadPosition.value - newPosition) > 0.001) {
    playheadPosition.value = newPosition
  }
})
const selectionStart = ref<number | null>(null)
const selectionEnd = ref<number | null>(null)
const timelineContentRef = ref<HTMLElement | null>(null)
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
  const newPPS = Math.min(maxZoom, pixelsPerSecond.value + zoomStep)
  
  if (newPPS !== oldPPS && timelineViewportWidth.value > 0) {
    // Update zoom
    pixelsPerSecond.value = newPPS
    
    // Adjust timelineStart so playhead ends up at the center of the viewport
    // Center pixel position = viewportWidth / 2
    // playheadTime = timelineStart + (centerPixel / newPPS)
    // timelineStart = playheadTime - (centerPixel / newPPS)
    const centerPixel = timelineViewportWidth.value / 2
    const newStart = playheadPosition.value - (centerPixel / newPPS)
    timelineStart.value = Math.max(0, newStart)
    updateTimelineEnd()
  }
}

const zoomOut = () => {
  const oldPPS = pixelsPerSecond.value
  const newPPS = Math.max(minZoom, pixelsPerSecond.value - zoomStep)
  
  if (newPPS !== oldPPS && timelineViewportWidth.value > 0) {
    // Update zoom
    pixelsPerSecond.value = newPPS
    
    // Adjust timelineStart so playhead ends up at the center of the viewport
    // Center pixel position = viewportWidth / 2
    // playheadTime = timelineStart + (centerPixel / newPPS)
    // timelineStart = playheadTime - (centerPixel / newPPS)
    const centerPixel = timelineViewportWidth.value / 2
    const newStart = playheadPosition.value - (centerPixel / newPPS)
    timelineStart.value = Math.max(0, newStart)
    updateTimelineEnd()
  }
}

// Panning state
const isPanning = ref(false)
const panStartX = ref(0)
const panStartTime = ref(0)

const handleWheel = (event: WheelEvent) => {
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

const handleMouseDown = (event: MouseEvent) => {
  // Middle mouse button or space+left button for panning
  if (event.button === 1 || (event.button === 0 && event.shiftKey)) {
    event.preventDefault()
    isPanning.value = true
    panStartX.value = event.clientX
    panStartTime.value = timelineStart.value
  }
}

const handleMouseMove = (event: MouseEvent) => {
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

const updateViewportWidth = () => {
  if (timelineContentRef.value) {
    timelineViewportWidth.value = timelineContentRef.value.clientWidth
    updateTimelineEnd()
  }
}

let resizeObserver: ResizeObserver | null = null

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

const handleClipStartUpdate = (data: { trackId: number; clipId: number; newStartTime: number }) => {
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

const handlePlayheadUpdate = (time: number) => {
  playheadPosition.value = time
  emit('update:playhead', time)
}

const handleSelectionUpdate = (data: { start: number | null; end: number | null }) => {
  selectionStart.value = data.start
  selectionEnd.value = data.end
}

const handleRemoveVideoTrack = async (trackId: number) => {
  const mediaIds = removeVideoTrack(trackId)
  if (mediaIds.length > 0) {
    const removedCount = await removeMultipleMedia(mediaIds)
    console.log(`Removed ${removedCount} media files from backend`)
  }
}

const handleRemoveAudioTrack = async (trackId: number) => {
  const mediaIds = removeAudioTrack(trackId)
  if (mediaIds.length > 0) {
    const removedCount = await removeMultipleMedia(mediaIds)
    console.log(`Removed ${removedCount} media files from backend`)
  }
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
          @remove-track="handleRemoveVideoTrack"
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
          @remove-track="handleRemoveAudioTrack"
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

