<script setup>
import { ref, reactive } from 'vue'

const selectedClip = ref(null)

// Sample clip properties
const clipProperties = reactive({
  name: 'Audio Clip 1',
  volume: 100,
  pan: 0,
  startOffset: 0,
  fadeIn: 0,
  fadeOut: 0,
  reverse: false,
  gain: 0
})

const updateProperty = (prop, value) => {
  clipProperties[prop] = value
}
</script>

<template>
  <div class="properties-panel panel">
    <div class="panel-header">Properties</div>
    <div class="properties-content">
      <div class="properties-form">
        <div class="property-group">
          <label class="property-label">Name</label>
          <input 
            type="text" 
            v-model="clipProperties.name" 
            class="property-input"
            @input="updateProperty('name', $event.target.value)" />
        </div>
        
        <div class="property-group">
          <label class="property-label">Volume</label>
          <div class="property-slider-group">
            <input 
              type="range" 
              v-model="clipProperties.volume" 
              min="0" 
              max="200" 
              class="property-slider"
              @input="updateProperty('volume', $event.target.value)" />
            <span class="property-value">{{ clipProperties.volume }}%</span>
          </div>
        </div>

        <div class="property-group">
          <label class="property-label">Pan</label>
          <div class="property-slider-group">
            <input 
              type="range" 
              v-model="clipProperties.pan" 
              min="-100" 
              max="100" 
              class="property-slider"
              @input="updateProperty('pan', $event.target.value)" />
            <span class="property-value">{{ clipProperties.pan > 0 ? 'R' : clipProperties.pan < 0 ? 'L' : 'C' }} {{ Math.abs(clipProperties.pan) }}</span>
          </div>
        </div>

        <div class="property-group">
          <label class="property-label">Gain (dB)</label>
          <div class="property-slider-group">
            <input 
              type="range" 
              v-model="clipProperties.gain" 
              min="-24" 
              max="24" 
              class="property-slider"
              @input="updateProperty('gain', $event.target.value)" />
            <span class="property-value">{{ clipProperties.gain >= 0 ? '+' : '' }}{{ clipProperties.gain }} dB</span>
          </div>
        </div>

        <div class="property-group">
          <label class="property-label">Start Offset</label>
          <input 
            type="number" 
            v-model="clipProperties.startOffset" 
            min="0"
            step="0.1"
            class="property-input"
            @input="updateProperty('startOffset', $event.target.value)" />
        </div>

        <div class="property-group">
          <label class="property-label">Fade In (s)</label>
          <input 
            type="number" 
            v-model="clipProperties.fadeIn" 
            min="0"
            step="0.1"
            class="property-input"
            @input="updateProperty('fadeIn', $event.target.value)" />
        </div>

        <div class="property-group">
          <label class="property-label">Fade Out (s)</label>
          <input 
            type="number" 
            v-model="clipProperties.fadeOut" 
            min="0"
            step="0.1"
            class="property-input"
            @input="updateProperty('fadeOut', $event.target.value)" />
        </div>

        <div class="property-group">
          <label class="property-checkbox">
            <input 
              type="checkbox" 
              v-model="clipProperties.reverse"
              @change="updateProperty('reverse', $event.target.checked)" />
            <span>Reverse</span>
          </label>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.properties-panel {
  position: absolute;
  right: 0;
  top: 0;
  width: 250px;
  height: 100%;
  display: flex;
  flex-direction: column;
  border-left: 1px solid #2a2a2a;
  z-index: 10;
  background-color: #1e1e1e;
}

.properties-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.properties-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.property-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.property-label {
  font-size: 11px;
  color: #999;
  text-transform: uppercase;
  font-weight: 600;
}

.property-input {
  background-color: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  padding: 6px 8px;
  color: #ccc;
  font-size: 12px;
  outline: none;
  transition: all 0.2s;
}

.property-input:hover {
  border-color: #4a4a4a;
}

.property-input:focus {
  border-color: #4a90e2;
}

.property-slider-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.property-slider {
  flex: 1;
  height: 4px;
  background: #2a2a2a;
  border-radius: 2px;
  outline: none;
  -webkit-appearance: none;
}

.property-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 12px;
  height: 12px;
  background: #4a90e2;
  border-radius: 50%;
  cursor: pointer;
}

.property-slider::-moz-range-thumb {
  width: 12px;
  height: 12px;
  background: #4a90e2;
  border-radius: 50%;
  cursor: pointer;
  border: none;
}

.property-value {
  font-size: 11px;
  color: #999;
  min-width: 60px;
  text-align: right;
}

.property-checkbox {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 12px;
  color: #ccc;
}

.property-checkbox input[type="checkbox"] {
  width: 16px;
  height: 16px;
  cursor: pointer;
  accent-color: #4a90e2;
}

.no-selection {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: #666;
  font-size: 12px;
}
</style>
