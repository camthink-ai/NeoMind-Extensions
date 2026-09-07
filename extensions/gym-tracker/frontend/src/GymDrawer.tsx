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
      const r = requestAnimationFrame(() => setShown(true))
      return () => cancelAnimationFrame(r)
    }
    setShown(false)
    const t = setTimeout(() => setMounted(false), 200)
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

  return (
    <div className={`gym-drawer-root ${shown ? 'shown' : ''}`}>
      <div className="gym-drawer-backdrop" onClick={onClose} />
      <aside
        className="gym-drawer"
        style={{ width: `min(${width}px, 92vw)` }}
        role="dialog"
        aria-modal="true"
      >
        <div className="gym-drawer-head">
          <span className="gym-drawer-title">{title}</span>
          <button className="gym-ov-btn" onClick={onClose}>关闭</button>
        </div>
        <div className="gym-drawer-body">{children}</div>
      </aside>
    </div>
  )
}
