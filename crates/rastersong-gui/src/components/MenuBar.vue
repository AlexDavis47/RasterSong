<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const activeTab = ref(null)
const showFileMenu = ref(false)

const tabs = ['File', 'Options', 'Settings', 'Help']

const setActiveTab = (tab) => {
  if (tab === 'File') {
    showFileMenu.value = !showFileMenu.value
  } else {
    showFileMenu.value = false
    activeTab.value = tab
  }
}

const importVideo = async () => {
  try {
    const filePath = await invoke('open_video_dialog')
    if (filePath) {
      console.log('Selected video file:', filePath)
    } else {
      console.log('No video file selected')
    }
  } catch (error) {
    console.error('Error opening video dialog:', error)
  }
  showFileMenu.value = false
}

const importAudio = async () => {
  try {
    const filePath = await invoke('open_audio_dialog')
    if (filePath) {
      console.log('Selected audio file:', filePath)
    } else {
      console.log('No audio file selected')
    }
  } catch (error) {
    console.error('Error opening audio dialog:', error)
  }
  showFileMenu.value = false
}

// Close menu when clicking outside
const handleClickOutside = (event) => {
  if (!event.target.closest('.menu-item') && !event.target.closest('.file-menu')) {
    showFileMenu.value = false
  }
}

// Add click listener when component mounts
onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})
onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <div class="menu-bar">
    <div class="menu-item" 
         v-for="tab in tabs" 
         :key="tab" 
         :class="{ active: activeTab === tab || (tab === 'File' && showFileMenu) }"
         @click="setActiveTab(tab)">
      {{ tab }}
      <div v-if="tab === 'File' && showFileMenu" class="file-menu">
        <div class="menu-option" @click.stop="importVideo">Import Video</div>
        <div class="menu-option" @click.stop="importAudio">Import Audio</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.menu-bar {
  grid-area: menu;
  display: flex;
  align-items: center;
  background-color: #252525;
  border-bottom: 1px solid #2a2a2a;
  padding: 0 8px;
  gap: 2px;
  position: relative;
}

.menu-item {
  padding: 12px 16px;
  cursor: pointer;
  color: #ccc;
  font-size: 13px;
  transition: background-color 0.2s;
  user-select: none;
  position: relative;
}

.menu-item:hover {
  background-color: #2e2e2e;
}

.menu-item.active {
  background-color: #333;
  color: #fff;
}

.file-menu {
  position: absolute;
  top: 100%;
  left: 0;
  background-color: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  min-width: 160px;
  z-index: 1000;
  margin-top: 2px;
}

.menu-option {
  padding: 10px 16px;
  color: #ccc;
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.2s;
}

.menu-option:hover {
  background-color: #333;
  color: #fff;
}
</style>

