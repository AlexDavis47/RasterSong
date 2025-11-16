<script setup>
import { computed, ref, onMounted, onUnmounted } from 'vue'

const props = defineProps({
  clip: {
    type: Object,
    required: true,
    validator: (value) => {
      return value.id !== undefined &&
             value.name !== undefined &&
             value.start !== undefined &&
             value.duration !== undefined &&
             value.isCarrier !== undefined
    }
  },
  pixelsPerSecond: {
    type: Number,
    required: true,
    default: 20
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

const emit = defineEmits(['update:start'])

const isDragging = ref(false)
const dragStartX = ref(0)
const dragStartTime = ref(0)
const dragOffsetX = ref(0) // Local pixel offset during drag for smooth updates
const clipElement = ref(null)

const clipStyle = computed(() => {
  // During dragging, use the drag start position + offset for smooth visual feedback
  // This avoids waiting for prop updates to flow back down
  let clipStartTime = props.clip.start
  if (isDragging.value) {
    const deltaTime = dragOffsetX.value / props.pixelsPerSecond
    clipStartTime = dragStartTime.value + deltaTime
  }
  
  // Position clip relative to the viewport start
  const leftPosition = (clipStartTime - props.timelineStart) * props.pixelsPerSecond
  
  return {
    left: leftPosition + 'px',
    width: (props.clip.duration * props.pixelsPerSecond) + 'px'
  }
})

const handleMouseDown = (event) => {
  isDragging.value = true
  dragStartX.value = event.clientX
  dragStartTime.value = props.clip.start
  dragOffsetX.value = 0
  event.preventDefault()
  event.stopPropagation()
}

let lastUpdateTime = 0
const UPDATE_THROTTLE = 16 // ~60fps

const handleMouseMove = (event) => {
  if (!isDragging.value) return
  
  const deltaX = event.clientX - dragStartX.value
  dragOffsetX.value = deltaX // Update local offset immediately for smooth visual feedback
  
  // Throttle prop updates to avoid excessive re-renders
  const now = performance.now()
  if (now - lastUpdateTime >= UPDATE_THROTTLE) {
    const deltaTime = deltaX / props.pixelsPerSecond
    const newStartTime = Math.max(0, dragStartTime.value + deltaTime)
    
    emit('update:start', {
      clipId: props.clip.id,
      newStartTime: newStartTime
    })
    
    lastUpdateTime = now
  }
}

const handleMouseUp = () => {
  if (isDragging.value) {
    // Final update to ensure position is synced
    const deltaX = dragOffsetX.value
    const deltaTime = deltaX / props.pixelsPerSecond
    const newStartTime = Math.max(0, dragStartTime.value + deltaTime)
    
    emit('update:start', {
      clipId: props.clip.id,
      newStartTime: newStartTime
    })
  }
  
  isDragging.value = false
  dragOffsetX.value = 0
}

onMounted(() => {
  document.addEventListener('mousemove', handleMouseMove)
  document.addEventListener('mouseup', handleMouseUp)
})

onUnmounted(() => {
  document.removeEventListener('mousemove', handleMouseMove)
  document.removeEventListener('mouseup', handleMouseUp)
})
</script>

<template>
  <div
    ref="clipElement"
    :class="['audio-clip', { 'carrier-clip': clip.isCarrier, 'dragging': isDragging }]"
    :style="clipStyle"
    @mousedown="handleMouseDown">
    {{ clip.name }}
  </div>
</template>

<style scoped>
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
  cursor: move;
  user-select: none;
  transition: background 0.2s, border-color 0.2s, box-shadow 0.2s;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.audio-clip.dragging {
  cursor: grabbing;
  transition: none;
}

.audio-clip:hover:not(.dragging) {
  background: linear-gradient(135deg, #5aa0f2 0%, #4580cd 100%);
  border-color: #6ab0ff;
  box-shadow: 0 2px 8px rgba(74, 144, 226, 0.3);
}

.carrier-clip {
  background: linear-gradient(135deg, #e24a4a 0%, #bd3535 100%);
  border: 1px solid #f25a5a;
}

.carrier-clip:hover:not(.dragging) {
  background: linear-gradient(135deg, #f25a5a 0%, #cd4545 100%);
  border-color: #ff6a6a;
  box-shadow: 0 2px 8px rgba(226, 74, 74, 0.3);
}
</style>

