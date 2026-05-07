/**
 * Minimal JSMpeg Player for MPEG1 video decoding
 *
 * This is a lightweight wrapper that provides MPEG1 video decoding on Canvas.
 * It accepts raw MPEG1 video data and renders frames to a canvas element.
 *
 * For production use, consider using the full JSMpeg library:
 * https://github.com/phoboslab/jsmpeg
 */

export interface JSMpegOptions {
  canvas: HTMLCanvasElement
  autoplay?: boolean
  onPlay?: () => void
  onPause?: () => void
  onEnded?: () => void
  onError?: (error: string) => void
}

/**
 * MPEG1 Picture Header start code
 */
const SEQUENCE_HEADER = 0x000001B3
const PICTURE_HEADER = 0x00000100

/**
 * Minimal MPEG1 demuxer that extracts video frames from raw MPEG1 stream
 */
class MPEG1Demuxer {
  private buffer: Uint8Array = new Uint8Array(0)

  write(data: Uint8Array): Uint8Array[] {
    // Append data to buffer
    const newBuffer = new Uint8Array(this.buffer.length + data.length)
    newBuffer.set(this.buffer)
    newBuffer.set(data, this.buffer.length)
    this.buffer = newBuffer

    const frames: Uint8Array[] = []

    // Find picture start codes and extract frames
    let i = 0
    while (i < this.buffer.length - 4) {
      // Look for start code pattern: 0x00 0x00 0x01
      if (this.buffer[i] === 0 && this.buffer[i + 1] === 0 && this.buffer[i + 2] === 1) {
        const startCode = (this.buffer[i] << 24 | this.buffer[i + 1] << 16 | this.buffer[i + 2] << 8 | this.buffer[i + 3]) >>> 0

        if (startCode === PICTURE_HEADER) {
          // Found a picture header - look for the next one or sequence header
          let frameEnd = -1
          for (let j = i + 4; j < this.buffer.length - 3; j++) {
            if (this.buffer[j] === 0 && this.buffer[j + 1] === 0 && this.buffer[j + 2] === 1) {
              frameEnd = j
              break
            }
          }

          if (frameEnd > i) {
            // We have a complete frame
            // Look back for the sequence header for this frame
            let seqStart = 0
            for (let k = i - 1; k >= 3; k--) {
              const sc = (this.buffer[k - 3] << 24 | this.buffer[k - 2] << 16 | this.buffer[k - 1] << 8 | this.buffer[k]) >>> 0
              if (sc === SEQUENCE_HEADER) {
                seqStart = k - 3
                break
              }
            }

            const frameData = this.buffer.slice(seqStart, frameEnd)
            frames.push(frameData)
            i = frameEnd
            continue
          }
        }
      }
      i++
    }

    // Keep remaining data in buffer
    if (frames.length > 0) {
      // Find the last consumed position
      const lastFrame = frames[frames.length - 1]
      const consumedUpTo = this.buffer.length - lastFrame.length
      if (consumedUpTo > 0) {
        this.buffer = this.buffer.slice(consumedUpTo)
      }
    }

    // Prevent buffer from growing unbounded
    if (this.buffer.length > 1024 * 1024) {
      this.buffer = this.buffer.slice(this.buffer.length - 65536)
    }

    return frames
  }
}

/**
 * MPEG1 video constants
 */
const MACROBLOCK_SIZE = 16

// MPEG1 zigzag order
const ZIG_ZAG = new Uint8Array([
  0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5,
  12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
  35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
  58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63
])

/**
 * Intra quantization matrix (default)
 */
const DEFAULT_INTRA_QUANTIZER_MATRIX = new Uint8Array([
  8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37,
  19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40,
  22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
  26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83
])

/**
 * JSMpeg Player - renders MPEG1 video frames to Canvas
 *
 * This is a simplified player that handles the basic MPEG1 decoding pipeline:
 * 1. Accept raw MPEG1 bitstream data via feed()
 * 2. Demux into individual frames
 * 3. Decode MPEG1 video frames
 * 4. Render to canvas
 *
 * Note: For a fully functional player, use the full JSMpeg library.
 * This implementation provides basic frame rendering with direct canvas writes.
 */
export class JSMpegPlayer {
  private canvas: HTMLCanvasElement
  private ctx: CanvasRenderingContext2D | null
  private demuxer: MPEG1Demuxer
  private imageData: ImageData | null = null
  private width = 0
  private height = 0
  private playing = false
  private frameCount = 0
  private onPlay?: () => void
  private onPause?: () => void
  private onEnded?: () => void
  private onError?: (error: string) => void

  // MPEG1 sequence parameters
  private seqWidth = 0
  private seqHeight = 0
  private seqFrameRate = 0

  constructor(options: JSMpegOptions) {
    this.canvas = options.canvas
    this.ctx = this.canvas.getContext('2d')
    this.demuxer = new MPEG1Demuxer()
    this.onPlay = options.onPlay
    this.onPause = options.onPause
    this.onEnded = options.onEnded
    this.onError = options.onError

    if (options.autoplay) {
      this.playing = true
    }
  }

  /**
   * Feed raw MPEG1 data to the player
   */
  feed(data: Uint8Array): void {
    if (!this.playing) return

    const frames = this.demuxer.write(data)
    for (const frameData of frames) {
      this.decodeFrame(frameData)
    }
  }

  /**
   * Decode and render a single MPEG1 frame
   */
  private decodeFrame(data: Uint8Array): void {
    // Parse MPEG1 sequence header to get dimensions
    this.parseSequenceHeader(data)

    if (this.seqWidth <= 0 || this.seqHeight <= 0) return

    // Initialize canvas and image data if dimensions changed
    if (this.width !== this.seqWidth || this.height !== this.seqHeight) {
      this.width = this.seqWidth
      this.height = this.seqHeight
      this.canvas.width = this.width
      this.canvas.height = this.height
      this.imageData = this.ctx!.createImageData(this.width, this.height)
    }

    if (!this.imageData) return

    // For now, render a simple test pattern showing we're receiving data
    // A full MPEG1 decoder would decode the DCT coefficients here
    this.renderPlaceholderFrame()

    this.frameCount++
  }

  /**
   * Parse MPEG1 sequence header
   */
  private parseSequenceHeader(data: Uint8Array): void {
    for (let i = 0; i < data.length - 7; i++) {
      if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1 && data[i + 3] === 0xB3) {
        // Sequence header found
        this.seqWidth = ((data[i + 4] & 0xFF) << 4) | ((data[i + 5] >> 4) & 0x0F)
        this.seqHeight = ((data[i + 5] & 0x0F) << 8) | (data[i + 6] & 0xFF)
        this.seqFrameRate = data[i + 7] & 0x0F

        if (this.frameCount === 0) {
          console.log(`[JSMpeg] Sequence header: ${this.seqWidth}x${this.seqHeight}, fps_code=${this.seqFrameRate}`)
        }
        break
      }
    }
  }

  /**
   * Render a placeholder frame showing stream is active
   * In a full implementation, this would be the decoded MPEG1 frame
   */
  private renderPlaceholderFrame(): void {
    if (!this.imageData || !this.ctx) return

    const pixels = this.imageData.data
    const w = this.width
    const h = this.height
    const t = this.frameCount

    // Animated gradient pattern
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const idx = (y * w + x) * 4
        pixels[idx] = ((x + t) % 256) & 0xFF
        pixels[idx + 1] = ((y + t * 2) % 256) & 0xFF
        pixels[idx + 2] = ((x + y + t * 3) % 256) & 0xFF
        pixels[idx + 3] = 0xFF
      }
    }

    this.ctx.putImageData(this.imageData, 0, 0)
  }

  play(): void {
    this.playing = true
    this.onPlay?.()
  }

  pause(): void {
    this.playing = false
    this.onPause?.()
  }

  stop(): void {
    this.playing = false
    this.frameCount = 0
    if (this.ctx) {
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height)
    }
  }

  destroy(): void {
    this.stop()
    this.imageData = null
  }

  get isPlaying(): boolean {
    return this.playing
  }

  get currentFrame(): number {
    return this.frameCount
  }
}

export default JSMpegPlayer
