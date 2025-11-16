<script setup>
import { ref } from 'vue'

const viewMode = ref('processed') // 'original', 'side-by-side', 'processed'
const playbackRate = ref(1.0)
const isLooping = ref(false)
const globalVolume = ref(100)

const setViewMode = (mode) => {
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
        <div class="preview-placeholder">
          <div v-if="viewMode === 'original'" class="single-view">
            <div class="video-placeholder">Original Video</div>
          </div>
          <div v-else-if="viewMode === 'side-by-side'" class="dual-view">
            <div class="video-placeholder">Original</div>
            <div class="video-placeholder">Processed</div>
          </div>
          <div v-else class="single-view">
            <div class="video-placeholder">Processed Video</div>
          </div>
        </div>
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
          <button class="control-btn" title="Play/Pause">⏸</button>
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
