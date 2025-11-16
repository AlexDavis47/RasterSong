<script setup>
import TimelineClip from './TimelineClip.vue'

const props = defineProps({
  track: {
    type: Object,
    required: true,
    validator: (value) => {
      return value.id !== undefined &&
             value.label !== undefined &&
             value.clips !== undefined &&
             Array.isArray(value.clips) &&
             value.isCarrier !== undefined
    }
  },
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
  }
})

const emit = defineEmits(['update:clip-start'])

const handleClipStartUpdate = (data) => {
  emit('update:clip-start', {
    trackId: props.track.id,
    ...data
  })
}
</script>

<template>
  <div :class="['track', { 'carrier-track': track.isCarrier }]">
    <div class="track-label">{{ track.label }}</div>
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
  padding: 8px;
  background-color: #1a1a1a;
  border-right: 1px solid #2a2a2a;
  font-size: 11px;
  color: #888;
  display: flex;
  align-items: center;
  flex-shrink: 0;
  z-index: 10;
  position: relative;
}

.track-content {
  flex: 1;
  position: relative;
  background-color: #1e1e1e;
  min-width: 0;
  overflow: hidden;
}
</style>

