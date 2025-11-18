import { ref } from 'vue'

// Types
export interface MediaInfo {
  id: string
  path: string
  duration: number
}

export interface Clip {
  id: number
  name: string
  start: number
  duration: number
  isCarrier: boolean
  mediaId?: string
  mediaPath?: string
}

export interface Track {
  id: number
  label: string
  isCarrier: boolean
  clips: Clip[]
}

// Global state
const videoTracks = ref<Track[]>([])
const audioTracks = ref<Track[]>([])
let nextTrackId = 0
let nextClipId = 0

export function useTimelineStore() {
  const addVideoClip = (mediaInfo: MediaInfo) => {
    // Create a new track for this video
    const trackId = nextTrackId++
    const clipId = nextClipId++
    
    const newTrack: Track = {
      id: trackId,
      label: `Video ${trackId + 1}`,
      isCarrier: true,
      clips: [
        {
          id: clipId,
          name: mediaInfo.path.split(/[\\/]/).pop() || 'Video Clip',
          start: 0,
          duration: mediaInfo.duration,
          isCarrier: true,
          mediaId: mediaInfo.id,
          mediaPath: mediaInfo.path
        }
      ]
    }
    
    videoTracks.value.push(newTrack)
  }
  
  const addAudioClip = (mediaInfo: MediaInfo) => {
    // Create a new track for this audio
    const trackId = nextTrackId++
    const clipId = nextClipId++
    
    const newTrack: Track = {
      id: trackId,
      label: `Audio ${trackId + 1}`,
      isCarrier: false,
      clips: [
        {
          id: clipId,
          name: mediaInfo.path.split(/[\\/]/).pop() || 'Audio Clip',
          start: 0,
          duration: mediaInfo.duration,
          isCarrier: false,
          mediaId: mediaInfo.id,
          mediaPath: mediaInfo.path
        }
      ]
    }
    
    audioTracks.value.push(newTrack)
  }
  
  const removeVideoTrack = (trackId: number): string[] => {
    const track = videoTracks.value.find(t => t.id === trackId)
    if (!track) return []
    
    // Collect media IDs from clips that need to be removed from backend
    const mediaIds = track.clips
      .filter(clip => clip.mediaId)
      .map(clip => clip.mediaId as string)
    
    // Remove the track
    videoTracks.value = videoTracks.value.filter(t => t.id !== trackId)
    
    return mediaIds
  }
  
  const removeAudioTrack = (trackId: number): string[] => {
    const track = audioTracks.value.find(t => t.id === trackId)
    if (!track) return []
    
    // Collect media IDs from clips that need to be removed from backend
    const mediaIds = track.clips
      .filter(clip => clip.mediaId)
      .map(clip => clip.mediaId as string)
    
    // Remove the track
    audioTracks.value = audioTracks.value.filter(t => t.id !== trackId)
    
    return mediaIds
  }
  
  return {
    videoTracks,
    audioTracks,
    addVideoClip,
    addAudioClip,
    removeVideoTrack,
    removeAudioTrack
  }
}

