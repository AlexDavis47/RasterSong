<script setup lang="ts">
import { ref, watch, computed, onUnmounted } from 'vue'
import { getFrameAtTimestamp, getFrameBoundaries, displayFrameOnCanvas } from '../utils/media'
import { useTimelineStore } from '../composables/useTimelineStore'

const props = defineProps<{
  playheadPosition: number
}>()

const emit = defineEmits<{
  'update:playhead': [time: number]
}>()

const { videoTracks, audioTracks } = useTimelineStore()

const viewMode = ref('processed') // 'original', 'side-by-side', 'processed'
const playbackRate = ref(1.0)
const isLooping = ref(false)
const globalVolume = ref(100)
const isPlaying = ref(false)

// Canvas refs
const canvasRef = ref<HTMLCanvasElement | null>(null)
const isLoadingFrame = ref(false)
const frameError = ref<string | null>(null)

// Track current frame boundaries to avoid unnecessary decodes
const currentFrameBoundaries = ref<{ start: number; end: number } | null>(null)

// Playback loop
let playbackAnimationFrame: number | null = null
let lastPlaybackTime: number | null = null
let expectedNextPosition: number | null = null

// Get the first video track's media ID (for now, we'll just use the first video)
const currentMediaId = computed(() => {
  if (videoTracks.value.length === 0) return null
  const firstTrack = videoTracks.value[0]
  if (firstTrack.clips.length === 0) return null
  return firstTrack.clips[0].mediaId
})

// Reset frame boundaries when media changes
watch(currentMediaId, () => {
  currentFrameBoundaries.value = null
})

// Calculate maximum duration from all tracks
const maxDuration = computed(() => {
  let max = 0
  for (const track of [...videoTracks.value, ...audioTracks.value]) {
    for (const clip of track.clips) {
      const endTime = clip.start + clip.duration
      if (endTime > max) {
        max = endTime
      }
    }
  }
  return max > 0 ? max : 60 // Default to 60s if no clips
})

// Playback loop function
const playbackLoop = (timestamp: number) => {
  if (!isPlaying.value) {
    playbackAnimationFrame = null
    expectedNextPosition = null
    return
  }

  if (lastPlaybackTime === null) {
    lastPlaybackTime = timestamp
    expectedNextPosition = props.playheadPosition
    playbackAnimationFrame = requestAnimationFrame(playbackLoop)
    return
  }

  // Check if playhead was moved externally (e.g., manual seek)
  // If the current position differs significantly from what we expected, reset the timer
  if (expectedNextPosition !== null) {
    const positionDiff = Math.abs(props.playheadPosition - expectedNextPosition)
    // If difference is more than 0.1 seconds, assume external seek
    if (positionDiff > 0.1) {
      lastPlaybackTime = timestamp
      expectedNextPosition = props.playheadPosition
    }
  }

  const deltaTime = (timestamp - lastPlaybackTime) / 1000 // Convert to seconds
  const newPosition = props.playheadPosition + (deltaTime * playbackRate.value)

  // Check if we've reached the end
  if (newPosition >= maxDuration.value) {
    if (isLooping.value) {
      // Loop back to start
      emit('update:playhead', 0)
      expectedNextPosition = 0
    } else {
      // Stop playback
      isPlaying.value = false
      emit('update:playhead', maxDuration.value)
      expectedNextPosition = null
    }
    lastPlaybackTime = null
    playbackAnimationFrame = null
    return
  }

  emit('update:playhead', newPosition)
  expectedNextPosition = newPosition
  lastPlaybackTime = timestamp
  playbackAnimationFrame = requestAnimationFrame(playbackLoop)
}

// Start playback
const startPlayback = () => {
  if (isPlaying.value) return
  isPlaying.value = true
  lastPlaybackTime = null
  expectedNextPosition = null
  playbackAnimationFrame = requestAnimationFrame(playbackLoop)
}

// Stop playback
const stopPlayback = () => {
  isPlaying.value = false
  lastPlaybackTime = null
  expectedNextPosition = null
  if (playbackAnimationFrame !== null) {
    cancelAnimationFrame(playbackAnimationFrame)
    playbackAnimationFrame = null
  }
}

// Toggle play/pause
const togglePlayPause = () => {
  if (isPlaying.value) {
    stopPlayback()
  } else {
    startPlayback()
  }
}

// Cleanup on unmount
onUnmounted(() => {
  stopPlayback()
})

// Watch playhead position and load frames only when crossing frame boundaries
watch(
  [() => props.playheadPosition, currentMediaId],
  async ([newPosition, mediaId]) => {
    console.log('PreviewPanel watcher triggered:', { newPosition, mediaId })
    
    if (!canvasRef.value || !mediaId) {
      console.log('Skipping frame load:', { hasCanvas: !!canvasRef.value, mediaId })
      currentFrameBoundaries.value = null
      return
    }
    
    // Don't request a new frame if we're already loading one
    if (isLoadingFrame.value) {
      console.log('Frame already loading, skipping request')
      return
    }
    
    // First, check if we're still within the current frame boundaries (fast check)
    if (currentFrameBoundaries.value) {
      const { start, end } = currentFrameBoundaries.value
      if (newPosition >= start && newPosition < end) {
        console.log(`Still in same frame [${start.toFixed(3)}s, ${end.toFixed(3)}s), skipping decode`)
        return
      }
    }
    
    // Get frame boundaries for the new position to verify we've crossed into a new frame
    let frameBoundaries: { start: number; end: number } | null = null
    try {
      frameBoundaries = await getFrameBoundaries(mediaId, newPosition)
      console.log(`Frame boundaries for ${newPosition}s: [${frameBoundaries.start.toFixed(3)}s, ${frameBoundaries.end.toFixed(3)}s)`)
      
      // Double-check: if we have current boundaries and the new boundaries are the same, skip
      if (currentFrameBoundaries.value) {
        const { start, end } = currentFrameBoundaries.value
        if (frameBoundaries.start === start && frameBoundaries.end === end) {
          console.log(`Still in same frame [${start.toFixed(3)}s, ${end.toFixed(3)}s), skipping decode`)
          // Update boundaries in case of floating point precision issues
          currentFrameBoundaries.value = frameBoundaries
          return
        }
      }
    } catch (error) {
      console.error('Failed to get frame boundaries:', error)
      // Fall back to decoding anyway
    }
    
    // We've crossed into a new frame, decode it
    isLoadingFrame.value = true
    frameError.value = null
    
    try {
      console.log(`Loading frame at timestamp: ${newPosition}s for media ${mediaId}`)
      const frame = await getFrameAtTimestamp(mediaId, newPosition)
      console.log(`Frame loaded: ${frame.width}x${frame.height} at ${frame.timestamp}s`)
      displayFrameOnCanvas(canvasRef.value, frame)
      
      // Update current frame boundaries
      if (frameBoundaries) {
        currentFrameBoundaries.value = frameBoundaries
      } else {
        // Fallback: estimate frame boundaries from FPS (if we can't get them from backend)
        // This shouldn't happen, but just in case
        currentFrameBoundaries.value = {
          start: frame.timestamp,
          end: frame.timestamp + (1.0 / 30.0) // Assume 30fps as fallback
        }
      }
    } catch (error) {
      console.error('Failed to load frame:', error)
      frameError.value = `Failed to load frame: ${error}`
      currentFrameBoundaries.value = null
    } finally {
      isLoadingFrame.value = false
    }
  },
  { 
    immediate: true,
    flush: 'post' // Ensure DOM updates before running
  }
)

const setViewMode = (mode: string) => {
  viewMode.value = mode
}

const toggleLoop = () => {
  isLooping.value = !isLooping.value
}

const playbackRates = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0]
</script>

<template>
  <div class="preview-panel panel">
    <div class="panel-header">Preview</div>
    <div class="preview-content">
      <div class="preview-area" :class="viewMode">
        <!-- Canvas for video preview -->
        <canvas 
          v-if="currentMediaId"
          ref="canvasRef" 
          class="video-canvas"
        />
        <div v-else class="preview-placeholder">
          <div class="single-view">
            <div class="video-placeholder">
              {{ isLoadingFrame ? 'Loading frame...' : 'Import a video to preview' }}
            </div>
          </div>
        </div>
        <!-- Debug info overlay -->
        <div v-if="currentMediaId" class="debug-info">
          Playhead: {{ playheadPosition.toFixed(2) }}s
          {{ isLoadingFrame ? '(loading...)' : '' }}
        </div>
        <div v-if="frameError" class="error-message">{{ frameError }}</div>
      </div>
      <div class="preview-controls">
        <div class="view-mode-toggle">
          <button 
            :class="{ active: viewMode === 'original' }"
            @click="setViewMode('original')"
            class="mode-btn"
            title="Original">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <rect x="2" y="2" width="12" height="12" stroke="currentColor" stroke-width="1.5" fill="none"/>
            </svg>
          </button>
          <button 
            :class="{ active: viewMode === 'side-by-side' }"
            @click="setViewMode('side-by-side')"
            class="mode-btn"
            title="Side-by-Side">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <rect x="1" y="2" width="7" height="12" stroke="currentColor" stroke-width="1.5" fill="none"/>
              <rect x="8" y="2" width="7" height="12" stroke="currentColor" stroke-width="1.5" fill="none"/>
            </svg>
          </button>
          <button 
            :class="{ active: viewMode === 'processed' }"
            @click="setViewMode('processed')"
            class="mode-btn"
            title="Processed">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <rect x="2" y="2" width="12" height="12" stroke="currentColor" stroke-width="1.5" fill="none"/>
              <path d="M4 6 L8 10 L12 6" stroke="currentColor" stroke-width="1.5" fill="none"/>
            </svg>
          </button>
        </div>
        <div class="playback-controls">
          <button 
            class="control-btn" 
            :class="{ active: isPlaying }"
            @click="togglePlayPause"
            title="Play/Pause">
            <svg v-if="isPlaying" width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
              <rect x="4" y="2" width="3" height="10" />
              <rect x="7" y="2" width="3" height="10" />
            </svg>
            <svg v-else width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
              <path d="M4 2 L12 7 L4 12 Z" />
            </svg>
          </button>
          <button 
            class="control-btn"
            :class="{ active: isLooping }"
            @click="toggleLoop"
            title="Loop">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M7 2 L10 5 L7 8 M7 12 L4 9 L7 6" stroke="currentColor" stroke-width="1.5" fill="none"/>
              <circle cx="7" cy="7" r="5" stroke="currentColor" stroke-width="1.5" fill="none"/>
            </svg>
          </button>
          <div class="playback-rate-control">
            <label>Rate:</label>
            <select v-model="playbackRate" class="rate-select">
              <option v-for="rate in playbackRates" :key="rate" :value="rate">
                {{ rate }}x
              </option>
            </select>
          </div>
          <div class="volume-control">
            <label>Volume:</label>
            <input 
              type="range" 
              v-model="globalVolume" 
              min="0" 
              max="200" 
              class="volume-slider"
              title="Global Volume" />
            <span class="volume-value">{{ globalVolume }}%</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.preview-panel {
  grid-area: preview;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.preview-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 16px;
  gap: 16px;
}

.preview-area {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: #0a0a0a;
  border: 1px solid #2a2a2a;
  border-radius: 4px;
  overflow: hidden;
}

.preview-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.single-view {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.dual-view {
  width: 100%;
  height: 100%;
  display: flex;
  gap: 2px;
}

.dual-view .video-placeholder {
  flex: 1;
}

.video-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
  color: #666;
  font-size: 18px;
  font-weight: 500;
}

.video-canvas {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  background: #000;
  display: block;
}

.debug-info {
  position: absolute;
  top: 10px;
  left: 10px;
  background: rgba(0, 0, 0, 0.7);
  color: #4a90e2;
  padding: 6px 10px;
  border-radius: 4px;
  font-size: 11px;
  font-family: 'Courier New', monospace;
  z-index: 100;
}

.error-message {
  position: absolute;
  bottom: 10px;
  left: 10px;
  right: 10px;
  background: rgba(255, 68, 68, 0.9);
  color: white;
  padding: 8px;
  border-radius: 4px;
  font-size: 12px;
  z-index: 100;
}

.preview-controls {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.view-mode-toggle {
  display: flex;
  gap: 2px;
  background-color: #2a2a2a;
  border-radius: 4px;
  padding: 2px;
}

.mode-btn {
  padding: 6px 8px;
  background-color: transparent;
  border: none;
  color: #999;
  cursor: pointer;
  border-radius: 2px;
  transition: all 0.2s;
  display: flex;
  align-items: center;
  justify-content: center;
}

.mode-btn:hover {
  background-color: #333;
  color: #ccc;
}

.mode-btn.active {
  background-color: #3a3a3a;
  color: #fff;
}

.playback-controls {
  display: flex;
  gap: 12px;
  align-items: center;
}

.playback-rate-control {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: #999;
}

.playback-rate-control label {
  font-size: 11px;
}

.rate-select {
  background-color: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  padding: 4px 8px;
  color: #ccc;
  font-size: 11px;
  cursor: pointer;
  outline: none;
}

.rate-select:hover {
  border-color: #4a4a4a;
}

.rate-select:focus {
  border-color: #4a90e2;
}

.control-btn {
  width: 32px;
  height: 32px;
  background-color: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  color: #ccc;
  cursor: pointer;
  font-size: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.control-btn:hover {
  background-color: #333;
  border-color: #4a4a4a;
  color: #fff;
}

.control-btn.active {
  background-color: #4a90e2;
  border-color: #5aa0f2;
  color: #fff;
}

.volume-control {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: #999;
}

.volume-control label {
  font-size: 11px;
  white-space: nowrap;
}

.volume-slider {
  width: 100px;
  height: 4px;
  background: #2a2a2a;
  border-radius: 2px;
  outline: none;
  -webkit-appearance: none;
}

.volume-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 12px;
  height: 12px;
  background: #4a90e2;
  border-radius: 50%;
  cursor: pointer;
}

.volume-slider::-moz-range-thumb {
  width: 12px;
  height: 12px;
  background: #4a90e2;
  border-radius: 50%;
  cursor: pointer;
  border: none;
}

.volume-value {
  font-size: 11px;
  color: #999;
  min-width: 45px;
  text-align: right;
}
</style>
