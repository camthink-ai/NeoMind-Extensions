/**
 * GymSelect — token-styled dropdown matching the host (shadcn) Select look.
 *
 * The extension bundle can't import host React components (separate React
 * instances), so this mirrors the visual contract instead: h-10 trigger,
 * rounded-md border-input bg-card, chevron, rounded-xl bg-popover shadow-xl
 * popup with a check on the active item. Colors ride the host's CSS custom
 * properties (--card/--popover/…) with dark fallbacks for standalone use.
 *
 * The popup is position:fixed from the trigger rect — zone rows live inside
 * an overflow:auto list that would clip an absolute popup.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

export interface GymSelectOption {
  value: string
  label: string
}

interface Props {
  value: string
  options: GymSelectOption[]
  onChange: (v: string) => void
  onClose?: () => void
  placeholder?: string
  /** open immediately (action-replacement dropdowns, e.g. 并入) */
  autoOpen?: boolean
  title?: string
  /** right-align the popup to the trigger's right edge (narrow rows near
   *  the card's right border — the inspector rows) */
  alignRight?: boolean
}

export function GymSelect({ value, options, onChange, onClose, placeholder, autoOpen, title, alignRight }: Props) {
  const [open, setOpen] = useState(!!autoOpen)
  const [active, setActive] = useState(0)
  const rootRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const popRefHolder = useRef<HTMLDivElement>(null)
  const [rect, setRect] = useState({ x: 0, y: 0, w: 0, h: 0, up: false })

  const close = useCallback(() => {
    setOpen(false)
    onClose?.()
  }, [onClose])

  const place = useCallback(() => {
    const r = triggerRef.current?.getBoundingClientRect()
    if (!r) return
    const popW = Math.max(r.width, 160)
    // right-aligned: popup's right edge hugs the trigger's right edge
    const x = alignRight ? Math.max(4, r.right - popW) : r.left
    setRect({
      x,
      y: r.top,
      w: r.width,
      h: r.height,
      up: window.innerHeight - r.bottom < 260 && r.top > 260,
    })
  }, [alignRight])

  useLayoutEffect(() => {
    if (open) place()
  }, [open, place])

  useEffect(() => {
    if (!open) return
    const idx = options.findIndex((o) => o.value === value)
    setActive(idx >= 0 ? idx : 0)
    const popRef = popRefHolder
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node
      // the popup is PORTALED to <body> (transformed grid ancestors turn
      // position:fixed into "relative to the card" — viewport coords from
      // getBoundingClientRect would place it wrong), so it is NOT under
      // rootRef; check both before calling it an outside click
      if (!rootRef.current?.contains(t) && !popRef.current?.contains(t)) close()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); close() }
      else if (e.key === 'ArrowDown') { e.preventDefault(); setActive((a) => Math.min(a + 1, options.length - 1)) }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)) }
      else if (e.key === 'Enter') {
        e.preventDefault()
        const o = options[active]
        if (o) { onChange(o.value); setOpen(false) }
      }
    }
    // any scroll (incl. the zone list) must reposition or close — fixed
    // popups detach from their anchor otherwise
    const onScroll = () => place()
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onKey, true)
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onScroll)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey, true)
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onScroll)
    }
  }, [open, options, value, active, close, onChange, place])

  const selected = options.find((o) => o.value === value)
  const popTop = rect.up ? rect.y - 6 : rect.y + rect.h + 6

  return (
    <div className="gym-sel" ref={rootRef} title={title}>
      <button
        ref={triggerRef}
        type="button"
        className={`gym-sel-trigger ${open ? 'open' : ''}`}
        onClick={() => (open ? close() : setOpen(true))}
      >
        <span className="gym-sel-value">{selected ? selected.label : (placeholder ?? '—')}</span>
        <svg className="gym-sel-chev" width="16" height="16" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {open && typeof document !== 'undefined' && createPortal(
        <div ref={popRefHolder} className="gym-sel-pop" style={{ left: rect.x, top: popTop, width: Math.max(rect.w, 160) }} role="listbox">
          {options.map((o, i) => (
            <div
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              className={`gym-sel-item ${i === active ? 'active' : ''} ${o.value === value ? 'selected' : ''}`}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => { onChange(o.value); setOpen(false) }}
            >
              <span className="gym-sel-check">{o.value === value ? '✓' : ''}</span>
              <span className="gym-sel-label">{o.label}</span>
            </div>
          ))}
        </div>,
        document.body
      )}
    </div>
  )
}
