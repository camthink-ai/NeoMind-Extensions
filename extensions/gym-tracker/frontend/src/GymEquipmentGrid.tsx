/**
 * GymEquipmentGrid — equipment-zone occupancy board.
 *
 * Combines `get_roi_zones` (persisted ROI zones) with `get_live_state`
 * (live track foot points) and computes occupancy client-side with the same
 * ray-casting rule the extension uses for its `gym.equipment_occupied.<zone>`
 * metrics — so the board always agrees with the metrics and the zone editor.
 */

import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  LiveState,
  Zone,
  fetchLiveState,
  fetchZones,
  injectStyles,
  pointInPolygon,
  fetchExtensionUiConfig,
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-eq-styles-v1'

interface ZoneView {
  zone: Zone
  count: number
  /** dwell-gated: someone held the zone past busySec */
  occupied: boolean
  /** people present but nobody held past busySec yet */
  warm: boolean
  holding: number
}

const DumbbellIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <path d="M6.5 6.5 17.5 17.5" />
    <path d="M21 21l-1-1" />
    <path d="M3 3l1 1" />
    <path d="M18 22l4-4" />
    <path d="M2 6l4-4" />
    <path d="M3 10l7-7" />
    <path d="M14 21l7-7" />
  </svg>
)

export const GymEquipmentGrid = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymEquipmentGrid(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    // dense boards can hide idle zones and show only what's in use
    const showIdle = config?.showIdle !== false
    // BUSY requires the SAME person (track) holding the zone this long —
    // raw presence only warms the cell. 5 s default per the gym's ask.
    const busySec = Math.min(600, Math.max(5, Number(config?.busySec) || 5))
    // global ui.language from the EXTENSION config (card lang overrides)
    const [gLang, setGLang] = useState<string | undefined>(undefined)
    useEffect(() => {
      let alive = true
      fetchExtensionUiConfig(extensionId).then((c: { ui?: { language?: string } }) => {
        if (alive) setGLang(c.ui?.language)
      })
      return () => { alive = false }
    }, [extensionId])
    const { t } = useLang(config as Record<string, unknown>, gLang)

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [zones, setZones] = useState<Zone[] | null>(null)
    const [state, setState] = useState<LiveState | null>(null)
    const [error, setError] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const [zr, sr] = await Promise.all([
        fetchZones(extensionId),
        fetchLiveState(extensionId),
      ])
      if (!mountedRef.current) return
      if (zr.success && zr.data) { setZones(zr.data.zones ?? []); setError(null) }
      else if (!zones) setError(zr.error || 'Failed to load zones')
      if (sr.success && sr.data) setState(sr.data)
    }, [extensionId, zones])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])

    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 2000)
      return () => clearInterval(id)
    }, [refresh])

    const views = useMemo<ZoneView[]>(() => {
      if (!zones) return []
      const tracks = state?.tracks ?? []
      return zones
        .filter((z) => (z.enabled === true || z.enabled === 1) && z.equipment_type !== 'exclusion')
        .map((zone) => {
          const inZone = tracks.filter(
            (t) => t.foot && pointInPolygon(t.foot.x, t.foot.y, zone.polygon)
          )
          // busy = someone has HELD this zone past the threshold (the
          // extension reports continuous dwell per track); merely being
          // inside only counts as "warm" so passers-by never flip a
          // machine to busy on their way through
          const holding = inZone.filter(
            (t) =>
              t.exercise?.zone === zone.id &&
              (t.exercise?.zone_hold ?? 0) >= busySec
          )
          const count = inZone.length
          return {
            zone,
            count,
            occupied: holding.length > 0,
            warm: count > 0 && holding.length === 0,
            holding: holding.length,
          }
        })
        .filter((v) => showIdle || v.occupied)
        .sort((a, b) => a.zone.name.localeCompare(b.zone.name))
    }, [zones, state, showIdle, busySec])

    const occupiedCount = views.filter((v) => v.occupied).length
    const totalInZones = views.reduce((s, v) => s + v.count, 0)

    return (
      <div ref={ref} className={`gym-eq ${className}`}>
        <div className="gym-eq-card">
          <div className="gym-eq-header">
            <div className="gym-eq-title">
              <DumbbellIcon />
              <span>Gym · {t('equipment')}</span>
            </div>
            <span className="gym-eq-summary">
              {occupiedCount}/{views.length} {t('busy')} · {totalInZones} {t('onGear')}
            </span>
          </div>

          {error ? (
            <div className="gym-eq-state">
              <span className="gym-eq-state-text">{error}</span>
            </div>
          ) : zones === null ? (
            <div className="gym-eq-state">
              <div className="gym-live-spinner" />
              <span className="gym-eq-state-text">Loading…</span>
            </div>
          ) : views.length === 0 ? (
            <div className="gym-eq-state">
              <DumbbellIcon />
              <span className="gym-eq-state-text">
                No zones — draw ROI zones in Gym · Live
              </span>
            </div>
          ) : (
            <div className="gym-eq-grid">
              {views.map(({ zone, count, occupied, warm }) => (
                <div
                  key={zone.id}
                  className={`gym-eq-cell ${occupied ? 'busy' : warm ? 'warm' : 'idle'}`}
                  title={
                    occupied
                      ? t('busy')
                      : warm
                        ? t('idle')
                        : t('idle')
                  }
                >
                  <span className={`gym-eq-dot ${occupied ? 'on' : warm ? 'warm' : ''}`} />
                  <div className="gym-eq-cell-body">
                    <span className="gym-eq-zone-name" title={zone.name}>
                      {zone.name}
                    </span>
                    <span className="gym-eq-zone-meta">
                      {zone.equipment_type || 'equipment'}
                      {count > 0 ? ` · ${count} ${t('people')}` : ''}
                    </span>
                  </div>
                  <span className={`gym-eq-count ${occupied ? 'on' : ''}`}>{count}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    )
  }
)

GymEquipmentGrid.displayName = 'GymEquipmentGrid'
