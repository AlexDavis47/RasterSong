<script setup>
import { ref } from 'vue'

const activeTab = ref('File')

const tabs = ['File', 'Options', 'Settings', 'Help']

const setActiveTab = (tab) => {
  activeTab.value = tab
}
</script>

<template>
  <div class="menu-bar">
    <div class="menu-item" v-for="tab in tabs" :key="tab" 
         :class="{ active: activeTab === tab }"
         @click="setActiveTab(tab)">
      {{ tab }}
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
}

.menu-item {
  padding: 12px 16px;
  cursor: pointer;
  color: #ccc;
  font-size: 13px;
  transition: background-color 0.2s;
  user-select: none;
}

.menu-item:hover {
  background-color: #2e2e2e;
}

.menu-item.active {
  background-color: #333;
  color: #fff;
}
</style>

