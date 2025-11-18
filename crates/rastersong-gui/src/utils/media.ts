import { invoke } from '@tauri-apps/api/core'

export interface FrameBoundaries {
  start: number
  end: number
}

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

/**
 * Get the frame boundaries for a given timestamp
 * 
 * Given a timestamp, returns the start and end time of the video frame
 * that contains that timestamp. This is useful for syncing audio samples
 * with video frames.
 * 
 * @param mediaId - The UUID string of the media
 * @param timestamp - Time in seconds
 * @returns Promise<FrameBoundaries> - Object with start and end times of the frame
 * @throws Error if the media has no video stream or is not found
 * 
 * @example
 * const boundaries = await getFrameBoundaries(mediaId, 1.5)
 * console.log(`Frame: ${boundaries.start}s to ${boundaries.end}s`)
 */
export async function getFrameBoundaries(
  mediaId: string,
  timestamp: number
): Promise<FrameBoundaries> {
  try {
    const result = await invoke<FrameBoundaries>('get_frame_boundaries', {
      id: mediaId,
      timestamp,
    })
    return result
  } catch (error) {
    console.error('Error getting frame boundaries:', error)
    throw error
  }
}

export interface VideoFrame {
  width: number
  height: number
  data: string // base64 encoded RGBA pixel data
  timestamp: number
}

/**
 * Get a decoded video frame at a specific timestamp
 * 
 * Returns a frame with RGBA pixel data encoded as base64, ready to display
 * on an HTML canvas.
 * 
 * @param mediaId - The UUID string of the media
 * @param timestamp - Time in seconds
 * @returns Promise<VideoFrame> - Frame with RGBA pixel data
 * @throws Error if decoding fails
 * 
 * @example
 * const frame = await getFrameAtTimestamp(mediaId, 1.5)
 * displayFrameOnCanvas(canvas, frame)
 */
export async function getFrameAtTimestamp(
  mediaId: string,
  timestamp: number
): Promise<VideoFrame> {
  try {
    const result = await invoke<VideoFrame>('get_frame_at_timestamp', {
      id: mediaId,
      timestamp,
    })
    return result
  } catch (error) {
    console.error('Error getting frame:', error)
    throw error
  }
}

/**
 * Display a video frame on an HTML canvas
 * 
 * @param canvas - HTML Canvas element
 * @param frame - Video frame data from getFrameAtTimestamp
 * 
 * @example
 * const canvas = document.getElementById('preview') as HTMLCanvasElement
 * const frame = await getFrameAtTimestamp(mediaId, 1.5)
 * displayFrameOnCanvas(canvas, frame)
 */
export function displayFrameOnCanvas(
  canvas: HTMLCanvasElement,
  frame: VideoFrame
): void {
  // Set canvas size to match frame
  canvas.width = frame.width
  canvas.height = frame.height

  const ctx = canvas.getContext('2d')
  if (!ctx) {
    throw new Error('Failed to get 2D context')
  }

  // Decode base64 data
  const binaryString = atob(frame.data)
  const bytes = new Uint8ClampedArray(binaryString.length)
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i)
  }

  // Create ImageData and draw to canvas
  const imageData = new ImageData(bytes, frame.width, frame.height)
  ctx.putImageData(imageData, 0, 0)
}

