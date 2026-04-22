import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import '@testing-library/jest-dom'
import { FaceRegistrationCard } from '../FaceRegistrationCard'

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

describe('FaceRegistrationCard', () => {
  const defaultProps = {
    extensionId: 'face-recognition',
    onClose: jest.fn(),
    onRegistered: jest.fn(),
  }

  beforeEach(() => {
    defaultProps.onClose.mockClear()
    defaultProps.onRegistered.mockClear()
  })

  // -----------------------------------------------------------------------
  // 1. Renders name input and file upload zone
  // -----------------------------------------------------------------------
  it('renders the name input field', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const nameInput = screen.getByPlaceholderText('输入姓名')
    expect(nameInput).toBeInTheDocument()
    expect(nameInput).toHaveAttribute('type', 'text')
  })

  it('renders the file upload drop zone', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const dropzone = screen.getByRole('button', { name: '上传人脸图片' })
    expect(dropzone).toBeInTheDocument()
  })

  it('renders the dialog title', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    expect(screen.getByText('注册人脸')).toBeInTheDocument()
  })

  // -----------------------------------------------------------------------
  // 2. Shows error when submitting without name (submit button disabled)
  // -----------------------------------------------------------------------
  it('disables submit button when name is empty', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const submitBtn = screen.getByText('注册')
    expect(submitBtn).toBeDisabled()
  })

  it('disables submit button when no image is selected', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    // Type a name but no image
    const nameInput = screen.getByPlaceholderText('输入姓名')
    await act(async () => {
      fireEvent.change(nameInput, { target: { value: 'Test User' } })
    })

    const submitBtn = screen.getByText('注册')
    expect(submitBtn).toBeDisabled()
  })

  // -----------------------------------------------------------------------
  // 3. Shows Chinese error messages for error codes
  // -----------------------------------------------------------------------
  it('displays DUPLICATE_NAME error in Chinese', async () => {
    mockFetch.mockImplementationOnce(() =>
      jsonOk({
        success: false,
        error: 'Duplicate name',
        error_code: 'DUPLICATE_NAME',
      }),
    )

    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    // Fill name and image to enable submit
    const nameInput = screen.getByPlaceholderText('输入姓名')
    await act(async () => {
      fireEvent.change(nameInput, { target: { value: 'Test' } })
    })

    // Simulate an image selection by triggering the file input
    const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['fake image data'], 'test.jpg', { type: 'image/jpeg' })

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [file] } })
    })

    // Wait for FileReader to process
    await act(async () => {
      jest.advanceTimersByTime(100)
    })

    // Click submit
    const submitBtn = screen.getByText('注册')
    await act(async () => {
      fireEvent.click(submitBtn)
    })

    await waitFor(() => {
      expect(screen.getByText('姓名已存在')).toBeInTheDocument()
    })
  })

  it('displays NO_FACE_DETECTED error in Chinese', async () => {
    mockFetch.mockImplementationOnce(() =>
      jsonOk({
        success: false,
        error: 'No face detected',
        error_code: 'NO_FACE_DETECTED',
      }),
    )

    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const nameInput = screen.getByPlaceholderText('输入姓名')
    await act(async () => {
      fireEvent.change(nameInput, { target: { value: 'Ghost' } })
    })

    const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['fake image data'], 'test.jpg', { type: 'image/jpeg' })

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [file] } })
    })

    await act(async () => {
      jest.advanceTimersByTime(100)
    })

    const submitBtn = screen.getByText('注册')
    await act(async () => {
      fireEvent.click(submitBtn)
    })

    await waitFor(() => {
      expect(screen.getByText('未检测到人脸')).toBeInTheDocument()
    })
  })

  // -----------------------------------------------------------------------
  // 4. Calls register_face command with name and image on submit
  // -----------------------------------------------------------------------
  it('calls register_face with correct arguments on successful submit', async () => {
    mockFetch.mockImplementationOnce(() =>
      jsonOk({ success: true, data: { face_id: 'face-123' } }),
    )

    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const nameInput = screen.getByPlaceholderText('输入姓名')
    await act(async () => {
      fireEvent.change(nameInput, { target: { value: 'Alice' } })
    })

    const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['fake image data'], 'alice.jpg', { type: 'image/jpeg' })

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [file] } })
    })

    // Wait for FileReader to process
    await act(async () => {
      jest.advanceTimersByTime(100)
    })

    const submitBtn = screen.getByText('注册')
    await act(async () => {
      fireEvent.click(submitBtn)
    })

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalledTimes(1)
      const [url, options] = mockFetch.mock.calls[0]
      expect(url).toContain('/extensions/face-recognition/command')
      expect(options.method).toBe('POST')
      const body = JSON.parse(options.body)
      expect(body.command).toBe('register_face')
      expect(body.args.name).toBe('Alice')
      expect(body.args.image).toBeTruthy()
    })
  })

  it('calls onRegistered and onClose on successful registration', async () => {
    mockFetch.mockImplementationOnce(() =>
      jsonOk({ success: true, data: { face_id: 'face-456' } }),
    )

    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const nameInput = screen.getByPlaceholderText('输入姓名')
    await act(async () => {
      fireEvent.change(nameInput, { target: { value: 'Bob' } })
    })

    const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['fake image data'], 'bob.jpg', { type: 'image/jpeg' })

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [file] } })
    })

    await act(async () => {
      jest.advanceTimersByTime(100)
    })

    const submitBtn = screen.getByText('注册')
    await act(async () => {
      fireEvent.click(submitBtn)
    })

    await waitFor(() => {
      expect(defaultProps.onRegistered).toHaveBeenCalled()
      expect(defaultProps.onClose).toHaveBeenCalled()
    })
  })

  // -----------------------------------------------------------------------
  // 5. Calls onClose when cancel button clicked
  // -----------------------------------------------------------------------
  it('calls onClose when cancel button is clicked', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const cancelBtn = screen.getByText('取消')
    await act(async () => {
      fireEvent.click(cancelBtn)
    })

    expect(defaultProps.onClose).toHaveBeenCalled()
  })

  // -----------------------------------------------------------------------
  // 6. Calls onClose when overlay is clicked
  // -----------------------------------------------------------------------
  it('calls onClose when overlay is clicked', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const overlay = screen.getByText('注册人脸').closest('.frc-reg-overlay')!
    expect(overlay).toBeInTheDocument()

    await act(async () => {
      fireEvent.click(overlay)
    })

    expect(defaultProps.onClose).toHaveBeenCalled()
  })

  // -----------------------------------------------------------------------
  // 7. Does not call onClose when dialog body is clicked
  // -----------------------------------------------------------------------
  it('does not call onClose when dialog body is clicked', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    const dialog = screen.getByText('注册人脸').closest('.frc-reg-dialog')!
    expect(dialog).toBeInTheDocument()

    await act(async () => {
      fireEvent.click(dialog)
    })

    expect(defaultProps.onClose).not.toHaveBeenCalled()
  })

  // -----------------------------------------------------------------------
  // 8. Shows cancel and register buttons
  // -----------------------------------------------------------------------
  it('renders cancel and register action buttons', async () => {
    await act(async () => {
      render(<FaceRegistrationCard {...defaultProps} />)
    })

    expect(screen.getByText('取消')).toBeInTheDocument()
    expect(screen.getByText('注册')).toBeInTheDocument()
  })
})
