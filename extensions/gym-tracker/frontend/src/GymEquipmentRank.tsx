/**
 * GymEquipmentRank — today's equipment usage ranking (horizontal bars).
 *
 * Reuses get_workout_summary's equipment aggregate: zone label + seconds +
 * reps. Pure presentational card, no new backend contract.
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  injectStyles,
  runExtensionCommand,
  fetchExtensionUiConfig,
  fetchZones,
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-rank-styles-v1'

interface EquipRow { zone_id: string; duration_sec: number; reps: number; exercise: string }
interface Summary { equipment: EquipRow[] }

export const GymEquipmentRank = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymEquipmentRank(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const topN = Math.min(20, Math.max(3, Number(config?.topN) || 8))
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

    const [rows, setRows] = useState<EquipRow[] | null>(null)
    const [noZones, setNoZones] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      // differentiate 'no usage yet' from 'zones not configured'
      fetchZones(extensionId).then((z: { success: boolean; data?: { zones?: unknown[] } }) => {
        if (mountedRef.current && z.success && z.data) setNoZones((z.data.zones ?? []).length === 0)
      })
      const r = await runExtensionCommand<Summary>(extensionId, 'get_workout_summary', {})
      if (!mountedRef.current) return
      if (r.success && r.data) setRows(r.data.equipment ?? [])
      else if (!r.success) setError(r.error || '加载失败')
    }, [extensionId])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])
    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 15000)
      return () => clearInterval(id)
    }, [refresh])

    const top = (rows ?? []).slice(0, topN)
    const max = Math.max(1, ...top.map((r) => r.duration_sec))
    const fmt = (s: number) =>
      s >= 3600 ? `${(s / 3600).toFixed(1)}h` : s >= 60 ? `${Math.round(s / 60)}m` : `${s}s`

    return (
      <div ref={ref} className={`gym-rank ${className}`}>
        <div className="gym-eq-card">
          <div className="gym-eq-header">
            <span className="gym-ov-title">{t('equipRank')} · {t('today')}</span>
            <span className="gym-ov-badge">{rows ? `${rows.length} {t('units')}` : '…'}</span>
          </div>
          <div className="gym-rank-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            {!error && rows && top.length === 0 && (
              <div className="gym-rank-empty">
                {noZones
                  ? <span className="gym-empty-warn">⚠ 未配置器械分区——在 Monitor 编辑模式中绘制分区后，此处开始统计</span>
                  : t('noUsageToday')}
              </div>
            )}
            {top.map((r, i) => (
              <div className="gym-rank-row" key={r.zone_id + i}>
                <span className="gym-rank-name" title={r.zone_id}>{r.zone_id}</span>
                <div className="gym-rank-track">
                  <div className="gym-rank-bar" style={{ width: `${Math.max(3, (r.duration_sec / max) * 100)}%` }} />
                </div>
                <span className="gym-rank-val">{fmt(r.duration_sec)}</span>
                {r.reps > 0 && <span className="gym-rank-reps">{r.reps}次</span>}
              </div>
            ))}
          </div>
        </div>
      </div>
    )
  },
)
