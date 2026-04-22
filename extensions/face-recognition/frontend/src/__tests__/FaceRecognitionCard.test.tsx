import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import '@testing-library/jest-dom'
import { FaceRecognitionCard } from '../index'

// ---------------------------------------------------------------------------
// Types (duplicated to avoid importing internals)
// ---------------------------------------------------------------------------

interface MockFetchOptions {
  method?: string
  headers?: Record<string, string>
  body?: string
}

// ---------------------------------------------------------------------------
// Mock helpers
// ---------------------------------------------------------------------------

const mockFetch = jest.fn()
global.fetch = mockFetch

function jsonOk(data: unknown, status = 200) {
  return Promise.resolve({
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(data),
  })
}

/** Default status response. */
const DEFAULT_STATUS = {
  model_loaded: true,
  total_bindings: 0,
  total_inferences: 42,
  total_faces_detected: 30,
  total_faces_recognized: 25,
  total_errors: 1,
}

/**
 * Set up a URL-aware mock for fetch that routes responses based on the URL
 * and request body, rather than call order. This avoids race conditions from
 * concurrent useEffect fetches.
 */
function setupFetchMock(overrides?: {
  devices?: { id: string; name: string }[]
  status?: unknown
  bindings?: unknown
  faces?: unknown
}) {
  const devices = overrides?.devices ?? []
  const status = overrides?.status ?? DEFAULT_STATUS
  const bindings = overrides?.bindings ?? []
  const faces = overrides?.faces ?? []

  mockFetch.mockImplementation((url: string, options: MockFetchOptions) => {
    // GET /api/devices
    if (url.includes('/devices') && !url.includes('/extensions/')) {
      return jsonOk({ data: { devices } })
    }

    // POST /api/extensions/.../command
    if (url.includes('/extensions/') && url.includes('/command') && options.body) {
      const body = JSON.parse(options.body)

      if (body.command === 'get_status') {
        return jsonOk({ success: true, data: status })
      }
      if (body.command === 'get_bindings') {
        return jsonOk({ success: true, data: { bindings } })
      }
      if (body.command === 'list_faces') {
        return jsonOk({ success: true, data: { faces } })
      }
      if (body.command === 'bind_device') {
        return jsonOk({ success: true, data: {} })
      }
      if (body.command === 'unbind_device') {
        return jsonOk({ success: true, data: {} })
      }
      if (body.command === 'delete_face') {
        return jsonOk({ success: true, data: {} })
      }
    }

    // Fallback
    return jsonOk({ success: false, error: 'Unknown endpoint' })
  })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeEach(() => {
  jest.useFakeTimers()
  mockFetch.mockReset()
  localStorage.clear()
  sessionStorage.clear()
})

afterEach(() => {
  jest.useRealTimers()
})

describe('FaceRecognitionCard', () => {
  // -----------------------------------------------------------------------
  // 1. Renders with title "Face Recognition"
  // -----------------------------------------------------------------------
  it('renders with the default title', async () => {
    setupFetchMock()

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Face Recognition')).toBeInTheDocument()
  })

  it('uses the custom title when provided', async () => {
    setupFetchMock()

    await act(async () => {
      render(<FaceRecognitionCard title="Custom Title" />)
    })

    expect(screen.getByText('Custom Title')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 2. Shows device selector dropdown
  // -----------------------------------------------------------------------
  it('shows the device selector dropdown', async () => {
    setupFetchMock({
      devices: [
        { id: 'cam-1', name: 'Front Camera' },
        { id: 'cam-2', name: 'Back Camera' },
      ],
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Select device...')).toBeInTheDocument()
    expect(screen.getByText('Device')).toBeInTheDocument()
  })

  it('lists devices in the dropdown when clicked', async () => {
    setupFetchMock({
      devices: [
        { id: 'cam-1', name: 'Front Camera' },
        { id: 'cam-2', name: 'Back Camera' },
      ],
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    // Click the device dropdown trigger
    const trigger = screen.getByText('Select device...')
    await act(async () => {
      fireEvent.click(trigger)
    })

    expect(screen.getByText('Front Camera')).toBeInTheDocument()
    expect(screen.getByText('Back Camera')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 3. Shows the Bind Device button (not bound state)
  // -----------------------------------------------------------------------
  it('shows Bind Device button when no device is bound', async () => {
    setupFetchMock()

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Bind Device')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 4. Calls executeCommand with correct args when binding device
  // -----------------------------------------------------------------------
  it('calls bind_device command when Bind Device is clicked', async () => {
    setupFetchMock({
      devices: [{ id: 'cam-1', name: 'Test Camera' }],
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    // Select a device from the dropdown
    const trigger = screen.getByText('Select device...')
    await act(async () => {
      fireEvent.click(trigger)
    })
    const deviceOption = screen.getByText('Test Camera')
    await act(async () => {
      fireEvent.click(deviceOption)
    })

    // Click Bind Device
    const bindBtn = screen.getByText('Bind Device')
    await act(async () => {
      fireEvent.click(bindBtn)
    })

    await waitFor(() => {
      const bindCalls = mockFetch.mock.calls.filter(
        (call: unknown[]) => {
          const opts = call[1] as MockFetchOptions | undefined
          return opts?.body?.includes('bind_device')
        },
      )
      expect(bindCalls.length).toBeGreaterThanOrEqual(1)
    })
  })

  // -----------------------------------------------------------------------
  // 5. Displays model status badge
  // -----------------------------------------------------------------------
  it('shows Ready badge when model is loaded but not bound', async () => {
    setupFetchMock({
      status: {
        model_loaded: true,
        total_bindings: 0,
        total_inferences: 0,
        total_faces_detected: 0,
        total_faces_recognized: 0,
        total_errors: 0,
      },
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Ready')).toBeInTheDocument()
  })

  it('shows No Model badge when model is not loaded', async () => {
    setupFetchMock({
      status: {
        model_loaded: false,
        total_bindings: 0,
        total_inferences: 0,
        total_faces_detected: 0,
        total_faces_recognized: 0,
        total_errors: 0,
      },
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('No Model')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 6. Shows registered faces with delete buttons
  // -----------------------------------------------------------------------
  it('displays registered faces when available', async () => {
    setupFetchMock({
      faces: [
        { id: 'face-1', name: 'Alice', registered_at: Date.now(), thumbnail: '' },
        { id: 'face-2', name: 'Bob', registered_at: Date.now(), thumbnail: '' },
      ],
    })

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Alice')).toBeInTheDocument()
    expect(screen.getByText('Bob')).toBeInTheDocument()
  })

  it('shows Register Face and Unbind buttons when device is bound', async () => {
    setupFetchMock({
      bindings: [
        {
          binding: { device_id: 'cam-1', metric_name: 'image', active: true, created_at: Date.now() },
          total_inferences: 0,
          total_recognized: 0,
          total_unknown: 0,
          last_faces: [],
        },
      ],
    })

    await act(async () => {
      render(
        <FaceRecognitionCard
          dataSource={{ type: 'device', extensionId: 'face-recognition', deviceId: 'cam-1' }}
        />,
      )
    })

    expect(screen.getByText('Register Face')).toBeInTheDocument()
    expect(screen.getByText('Unbind')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 7. Uses getDevices prop when provided
  // -----------------------------------------------------------------------
  it('uses getDevices prop instead of fetch when provided', async () => {
    const mockGetDevices = jest.fn().mockResolvedValue([
      { id: 'ext-cam', name: 'External Camera' },
    ])

    // Only mock the command endpoints (no devices fetch expected)
    mockFetch.mockImplementation((url: string, options: MockFetchOptions) => {
      if (url.includes('/extensions/') && url.includes('/command') && options.body) {
        const body = JSON.parse(options.body)
        if (body.command === 'get_status') return jsonOk({ success: true, data: DEFAULT_STATUS })
        if (body.command === 'get_bindings') return jsonOk({ success: true, data: { bindings: [] } })
        if (body.command === 'list_faces') return jsonOk({ success: true, data: { faces: [] } })
      }
      return jsonOk({ success: false, error: 'Unexpected endpoint' })
    })

    await act(async () => {
      render(<FaceRecognitionCard getDevices={mockGetDevices} />)
    })

    expect(mockGetDevices).toHaveBeenCalled()
    const deviceCalls = mockFetch.mock.calls.filter(
      (call: unknown[]) => {
        const fetchUrl = call[0] as string
        return typeof fetchUrl === 'string' && fetchUrl.includes('/devices') && !fetchUrl.includes('/extensions/')
      },
    )
    expect(deviceCalls.length).toBe(0)
  })

  // -----------------------------------------------------------------------
  // 8. Shows placeholder text when not bound
  // -----------------------------------------------------------------------
  it('shows placeholder text when no device is bound', async () => {
    setupFetchMock()

    await act(async () => {
      render(<FaceRecognitionCard />)
    })

    expect(screen.getByText('Bind a device to start')).toBeInTheDocument()
  })
})
