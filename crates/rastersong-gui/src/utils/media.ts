import { invoke } from '@tauri-apps/api/core'

/**
 * Remove media from the backend by its ID
 * @param mediaId - The UUID string of the media to remove
 * @returns Promise<boolean> - true if the media was found and removed, false otherwise
 */
export async function removeMedia(mediaId: string): Promise<boolean> {
  try {
    const result = await invoke<boolean>('remove_media', { id: mediaId })
    return result
  } catch (error) {
    console.error('Error removing media:', error)
    return false
  }
}

/**
 * Remove multiple media files from the backend
 * @param mediaIds - Array of UUID strings of media to remove
 * @returns Promise<number> - Number of media files successfully removed
 */
export async function removeMultipleMedia(mediaIds: string[]): Promise<number> {
  let removedCount = 0
  
  for (const mediaId of mediaIds) {
    const success = await removeMedia(mediaId)
    if (success) {
      removedCount++
    }
  }
  
  return removedCount
}

