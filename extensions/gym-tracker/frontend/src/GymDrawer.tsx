/**
 * GymDrawer — viewport-anchored right side drawer (record panels,
 * management surfaces). The extension bundle can't use the host's Sheet
 * component (separate React instances), so this mirrors the pattern:
 * fixed right panel + click-away backdrop + Esc to close, sliding in.
 *
 * z-index ladder inside the bundle: backdrop 230 < drawer 240 <
 * GymSelect popup 300 (a dropdown opened INSIDE the drawer must win).
 */
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { injectStyles } from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-drawer-styles-v1'

interface Props {
  open: boolean
  onClose: () => void
  title?: React.ReactNode
  children: React.ReactNode
  /** panel width in px (clamped to viewport) */
  width?: number
}

export function GymDrawer({ open, onClose, title, children, width = 380 }: Props) {
  const [mounted, setMounted] = useState(open)
  const [shown, setShown] = useState(false)
  const prevOpen = useRef(false)
  useEffect(() => injectStyles(STYLE_ID, STYLES), [])

  // keep the panel mounted during the close transition
  useEffect(() => {
    if (open) {
      setMounted(true)
      // NOT requestAnimationFrame — rAF is paused in non-composited
      // webviews (the IAB), and the drawer would stay translated off-screen
      const t = setTimeout(() => setShown(true), 20)
      return () => clearTimeout(t)
    }
    setShown(false)
    // no close transition anymore — unmount on the next macrotask is enough
    const t = setTimeout(() => setMounted(false), 0)
    return () => clearTimeout(t)
  }, [open])

  useEffect(() => {
    if (!open || !prevOpen.current) prevOpen.current = open
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  if (!mounted) return null

  // Portal to <body>: dashboard grid cells sit under transformed ancestors
  // (drag-and-drop wrappers), where position:fixed degenerates to
  // "relative to that card" — the drawer would be clipped/misplaced.
  if (typeof document === 'undefined') return null
  return createPortal(
    <div className={`gym-drawer-root ${shown ? 'shown' : ''}`}>
      <div
        className="gym-drawer-backdrop"
        style={{ opacity: shown ? 1 : 0 }}
        onClick={onClose}
      />
      <aside
        className="gym-drawer"
        style={{
          width: `min(${width}px, 92vw)`,
          // inline transform/opacity: a WKWebView style-matching quirk left
          // the class-driven transition stuck off-screen — inline wins
          transform: shown ? 'translateX(0)' : 'translateX(100%)',
        }}
        role="dialog"
        aria-modal="true"
      >
        <div className="gym-drawer-head">
          <span className="gym-drawer-title">{title}</span>
          <button className="gym-ov-btn" onClick={onClose}>Close</button>
        </div>
        <div className="gym-drawer-body">{children}</div>
      </aside>
    </div>,
    document.body
  )
}

/** Centered large modal variant — same portal/backdrop/Esc machinery. */
export function GymModal({ open, onClose, title, children, width = 900 }: {
  open: boolean
  onClose: () => void
  title?: React.ReactNode
  children: React.ReactNode
  width?: number
}) {
  const [mounted, setMounted] = useState(open)
  const prevOpen = useRef(false)
  useEffect(() => injectStyles(STYLE_ID, STYLES), [])
  useEffect(() => { setMounted(open) }, [open])
  useEffect(() => {
    if (!open || !prevOpen.current) prevOpen.current = open
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])
  if (!mounted || typeof document === 'undefined') return null
  return createPortal(
    <div className="gym-modal-root shown">
      <div className="gym-modal-backdrop" onClick={onClose} />
      <div className="gym-modal" style={{ width: `min(${width}px, 94vw)` }} role="dialog" aria-modal="true">
        <div className="gym-drawer-head">
          <span className="gym-drawer-title">{title}</span>
          <button className="gym-ov-btn" onClick={onClose}>Close</button>
        </div>
        <div className="gym-drawer-body">{children}</div>
      </div>
    </div>,
    document.body
  )
}
