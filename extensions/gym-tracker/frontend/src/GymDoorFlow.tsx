/**
 * GymDoorFlow — door entry/exit flow with persisted daily history.
 *
 * get_crossing_history aggregates the crossing_day table (written on every
 * counter flip since the persistence fix) across all counting lines; today's
 * row is overlaid with the LIVE in-memory tally so "now" reads current.
 */
import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  injectStyles,
  runExtensionCommand,
  fetchExtensionUiConfig
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-door-styles-v1'

const DoorIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" />
    <polyline points="10 17 15 12 10 7" />
    <line x1="15" y1="12" x2="3" y2="12" />
  </svg>
)

interface HistRow { day: number; in: number; out: number; net: number }
interface Flow {
  days: number
  today: { in: number; out: number; net: number }
  presence?: { live: number; drift: number }
  history: HistRow[]
}

/** CE-days (chrono num_days_from_ce) → "MM/DD 周X" */
function ceDayLabel(ceDay: number, withWeekday = true): string {
  // chrono epoch day = CE day − 719_163
  const d = new Date((ceDay - 719163) * 86400000)
  const md = `${d.getMonth() + 1}/${d.getDate()}`
  if (!withWeekday) return md
  return `${md} ${['Sun','Mon','Tue','Wed','Thu','Fri','Sat'][d.getDay()]}`
}

export const GymDoorFlow = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymDoorFlow(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const days = Math.min(90, Math.max(1, Number(config?.days) || 7))
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

    const [flow, setFlow] = useState<Flow | null>(null)
    const [error, setError] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const r = await runExtensionCommand<Flow>(extensionId, 'get_crossing_history', { days })
      if (!mountedRef.current) return
      if (r.success && r.data) { setFlow(r.data); setError(null) }
      else if (!r.success) setError(r.error || t('loadFailed'))
    }, [extensionId, days])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])
    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 10000)
      return () => clearInterval(id)
    }, [refresh])

    // full calendar window with zero-fill (days without a row were simply
    // not persisted / nobody crossed)
    const rows = useMemo(() => {
      if (!flow) return []
      const byDay = new Map(flow.history.map((r) => [r.day, r]))
      const todayCe = flow.history.length
        ? Math.max(...flow.history.map((r) => r.day))
        : null
      if (todayCe == null) return []
      const out: HistRow[] = []
      for (let d = todayCe - (days - 1); d <= todayCe; d++) {
        const r = byDay.get(d)
        out.push({ day: d, in: r?.in ?? 0, out: r?.out ?? 0, net: r?.net ?? 0 })
      }
      return out.reverse() // newest first, today on top
    }, [flow, days])

    const maxInOut = Math.max(1, ...rows.map((r) => Math.max(r.in, r.out)))

    return (
      <div ref={ref} className={`gym-door ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <div className="gym-traffic-title">
              <DoorIcon />
              <span>{t('doorTitle')} · {t('lastNDays', { n: days })}</span>
            </div>
            <span className="gym-ov-badge">
              {flow ? `${t('netIn')} ${flow.today.net >= 0 ? '+' : ''}${flow.today.net}` : '…'}
            </span>
            {flow?.presence && Math.abs(flow.presence.drift) > 1 && (
              <span className="gym-ov-badge warn" title={`Live tracking shows ${flow.presence.live} in gym — door counters drifted by ${flow.presence.drift}`}>
                ⚠ {flow.presence.live} live
              </span>
            )}
          </div>

          <div className="gym-door-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            {!error && flow && (
              <div className="gym-door-today">
                <div className="gym-door-today-item in">
                  <span className="gym-door-today-value">{flow.today.in}</span>
                  <span className="gym-door-today-label">{t('todayIn')} ↑</span>
                </div>
                <div className="gym-door-today-item out">
                  <span className="gym-door-today-value">{flow.today.out}</span>
                  <span className="gym-door-today-label">{t('todayOut')} ↓</span>
                </div>
                <div className="gym-door-today-item net">
                  <span className="gym-door-today-value">
                    {flow.today.net >= 0 ? '+' : ''}{flow.today.net}
                  </span>
                  <span className="gym-door-today-label">{t('netIn')}</span>
                </div>
              </div>
            )}

            {rows.length === 0 && !error && (
              <div className="gym-rank-empty">
                {t('noDoorRecords')}
              </div>
            )}

            {rows.length > 0 && (
              <div className="gym-door-hist">
                <div className="gym-door-hist-head">
                  <span className="gym-door-hist-side out">{t('out')} ↓</span>
                  <span className="gym-door-hist-mid">{t('date')}</span>
                  <span className="gym-door-hist-side in">{t('in')} ↑</span>
                </div>
                {rows.map((r) => (
                  <div key={r.day} className="gym-door-hist-row">
                    <div className="gym-door-cell out">
                      <span className="gym-door-cell-v">{r.out > 0 ? r.out : ''}</span>
                      <div
                        className="gym-door-cellbar out"
                        style={{ width: `${(r.out / maxInOut) * 100}%` }}
                        title={`out ${r.out}`}
                      />
                    </div>
                    <span className="gym-door-hist-mid" title={ceDayLabel(r.day)}>
                      {ceDayLabel(r.day)}
                    </span>
                    <div className="gym-door-cell in">
                      <div
                        className="gym-door-cellbar in"
                        style={{ width: `${(r.in / maxInOut) * 100}%` }}
                        title={`in ${r.in}`}
                      />
                      <span className="gym-door-cell-v">{r.in > 0 ? r.in : ''}</span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    )
  },
)

GymDoorFlow.displayName = 'GymDoorFlow'
