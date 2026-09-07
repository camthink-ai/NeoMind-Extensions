/**
 * GymAlerts — live safety/ops alert feed.
 *
 * get_alerts returns the extension's recent alert ring: fall-suspects
 * (torso lying sustained outside lying exercises) and long-occupancy
 * notices. 5 s poll keeps it near-live.
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  injectStyles,
  runExtensionCommand,
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-alerts-styles-v1'

interface AlertItem { ts: number; kind: string; level: string; track_id: number; message: string }
interface AlertsResp { alerts: AlertItem[] }

export const GymAlerts = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymAlerts(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const onlyWarn = config?.onlyWarn === true
    const { t } = useLang(config as Record<string, unknown>)

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [items, setItems] = useState<AlertItem[] | null>(null)
    const [error, setError] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const r = await runExtensionCommand<AlertsResp>(extensionId, 'get_alerts', {})
      if (!mountedRef.current) return
      if (r.success && r.data) setItems(r.data.alerts ?? [])
      else if (!r.success) setError(r.error || '加载失败')
    }, [extensionId])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])
    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 5000)
      return () => clearInterval(id)
    }, [refresh])

    const shown = (items ?? []).filter((a) => !onlyWarn || a.level === 'warn')
    const warnCount = shown.filter((a) => a.level === 'warn').length
    const fmt = (ts: number) => {
      const d = new Date(ts * 1000)
      return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}:${String(d.getSeconds()).padStart(2, '0')}`
    }

    return (
      <div ref={ref} className={`gym-alerts ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <span className="gym-ov-title">{t('alertsTitle')}</span>
            <span className={`gym-ov-badge ${warnCount > 0 ? 'warn' : 'ok'}`}>
              {items == null ? '…' : warnCount > 0 ? `${warnCount}` : t('allGood')}
            </span>
          </div>
          <div className="gym-alerts-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            {!error && shown.length === 0 && (
              <div className="gym-alerts-empty">
                <span className="gym-alerts-dot ok" />
                {t('noAlerts')}
              </div>
            )}
            {shown.map((a, i) => (
              <div className={`gym-alerts-row ${a.level}`} key={`${a.ts}-${i}`}>
                <span className={`gym-alerts-dot ${a.level}`} />
                <span className="gym-alerts-time">{fmt(a.ts)}</span>
                <span className="gym-alerts-msg">{a.message}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    )
  },
)
