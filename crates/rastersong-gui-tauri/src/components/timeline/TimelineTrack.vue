<script setup lang="ts">
import TimelineClip from './TimelineClip.vue'
import type { Track } from '../../composables/useTimelineStore'

const props = defineProps<{
  track: Track
  pixelsPerSecond: number
  viewportWidth: number
  timelineStart: number
  timelineEnd: number
}>()

const emit = defineEmits<{
  'update:clip-start': [data: { trackId: number; clipId: number; newStartTime: number }]
  'remove-track': [trackId: number]
}>()

const handleClipStartUpdate = (data: { clipId: number; newStartTime: number }) => {
  emit('update:clip-start', {
    trackId: props.track.id,
    ...data
  })
}

const handleRemoveTrack = () => {
  emit('remove-track', props.track.id)
}
</script>

<template>
  <div :class="['track', { 'carrier-track': track.isCarrier }]">
    <div class="track-label">
      <button class="remove-track-btn" @click="handleRemoveTrack" title="Remove track">×</button>
      <span class="track-label-text">{{ track.label }}</span>
    </div>
    <div class="track-content">
      <TimelineClip
        v-for="clip in track.clips"
        :key="clip.id"
        :clip="clip"
        :pixels-per-second="pixelsPerSecond"
        :timeline-start="timelineStart"
        :timeline-end="timelineEnd"
        @update:start="handleClipStartUpdate"
      />
    </div>
  </div>
</template>

<style scoped>
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
  padding: 4px 8px;
  background-color: #1a1a1a;
  border-right: 1px solid #2a2a2a;
  font-size: 11px;
  color: #888;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  z-index: 10;
  position: relative;
}

.track-label-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remove-track-btn {
  width: 16px;
  height: 16px;
  padding: 0;
  background-color: transparent;
  border: none;
  color: #666;
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 2px;
  flex-shrink: 0;
  transition: all 0.2s;
}

.remove-track-btn:hover {
  background-color: #ff4444;
  color: #fff;
}

.remove-track-btn:active {
  background-color: #cc0000;
}

.track-content {
  flex: 1;
  position: relative;
  background-color: #1e1e1e;
  min-width: 0;
  overflow: hidden;
}
</style>

