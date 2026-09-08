/**
 * GymWorkoutSummary — daily workout analytics card.
 *
 * Reads `get_workout_summary` (sessions + equipment_usage since local
 * midnight, or the picked day) and renders three visual layers:
 *  - hero stats row (total time / visits / unique members)
 *  - a 24h Gantt timeline: one lane per member, colored session blocks
 *  - an equipment-duration donut with a proportional legend
 *  - the raw session list beneath
 */

import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  runExtensionCommand,
  injectStyles,
  fetchExtensionUiConfig,
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-sum-styles-v2'

interface SessionRow {
  id: string
  member_id?: string | null
  member_name?: string | null
  started_at?: number | null
  ended_at?: number | null
  duration_sec: number
}

interface EquipmentRow {
  zone_id: string
  duration_sec: number
  reps: number
  exercise: string
}

interface Summary {
  since: number
  sessions: SessionRow[]
  equipment: EquipmentRow[]
  total_duration_sec: number
  visit_count: number
}

/** Local-midnight timestamp (sec) for a Date in the viewer's timezone. */
function localMidnight(d: Date): number {
  const copy = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  return Math.floor(copy.getTime() / 1000)
}

function fmtDuration(sec: number): string {
  if (sec <= 0) return '—'
  const h = Math.floor(sec / 3600)
  const m = Math.round((sec % 3600) / 60)
  return h > 0 ? `${h}h ${m}m` : `${m}m`
}

function fmtTime(ts: number | null | undefined): string {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  })
}

// Bilingual labels: EN first (product default), zh available by
// swapping the pick at the bottom.
const EXERCISE_LABELS: Record<string, [string, string]> = {
  treadmill_run: ['Treadmill', '跑步机'],
  walk: ['Walk', '步行'],
  run: ['Run', '跑步'],
  squat: ['Squat', '深蹲'],
  lunge: ['Lunge', '弓步'],
  deadlift: ['Deadlift', '硬拉'],
  bench: ['Bench press', '卧推'],
  pushup: ['Push-up', '俯卧撑'],
  chest_fly: ['Chest fly', '飞鸟'],
  crunch: ['Crunch', '卷腹'],
  situp: ['Sit-up', '仰卧起坐'],
  plank: ['Plank', '平板支撑'],
  pullup: ['Pull-up', '引体向上'],
  shoulder_press: ['Shoulder press', '肩推'],
  bicep_curl: ['Bicep curl', '弯举'],
  lateral_raise: ['Lateral raise', '侧平举'],
  dumbbell_row: ['Dumbbell row', '哑铃划船'],
  cycling: ['Cycling', '动感单车'],
  rowing: ['Rowing', '划船机'],
  elliptical: ['Elliptical', '椭圆机'],
  standing: ['Standing', '站立'],
  // expanded-equipment exercises (2026-09 audit)
  leg_press: ['Leg press', '腿举'],
  leg_extension: ['Leg extension', '腿屈伸'],
  leg_curl: ['Leg curl', '腿弯举'],
  hip_thrust: ['Hip thrust', '臀推'],
  incline_bench: ['Incline bench', '上斜卧推'],
  chest_press: ['Chest press', '坐推胸'],
  pec_deck: ['Pec deck', '蝴蝶机'],
  seated_row: ['Seated row', '坐姿划船'],
  lat_pulldown: ['Lat pulldown', '高位下拉'],
  preacher_curl: ['Preacher curl', '弯举凳'],
  triceps_pressdown: ['Triceps pressdown', '三头下压'],
  roman_chair: ['Roman chair', '罗马椅'],
  stair_climber: ['Stair climber', '爬楼机'],
  spin_bike: ['Spin bike', '动感单车'],
  kettlebell_swing: ['Kettlebell swing', '壶铃摆'],
  stretch: ['Stretch', '拉伸'],
  unknown: ['Unknown', '未识别'],
  pending: ['Detecting…', '识别中…'],
}

/** pick by active lang (module flag set from useLang) */
let exLabelLang: 'zh' | 'en' = 'en'
const exLabel = (name: string): string => {
  const pair = EXERCISE_LABELS[name]
  if (!pair) return name.replace(/_/g, ' ')
  return exLabelLang === 'zh' ? pair[1] : pair[0]
}

/** Distinct lane/donut colors that read on both light & dark themes. */
const PALETTE = [
  '#3b82f6', '#22c55e', '#f59e0b', '#ec4899',
  '#06b6d4', '#8b5cf6', '#ef4444', '#84cc16',
  '#f97316', '#14b8a6',
]

const ClockIcon = ({ className = '' }: { className?: string }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor"
    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="10" />
    <polyline points="12 6 12 12 16 14" />
  </svg>
)

const DAY_MS = 86400000

interface MemberLane {
  key: string
  name: string
  color: string
  total: number
  blocks: Array<{ id: string; start: number; end: number; dur: number }>
}

export const GymWorkoutSummary =
  forwardRef<HTMLDivElement, ExtensionComponentProps>(
    function GymWorkoutSummary(props, ref) {
      const { dataSource, className = '', config } = props
      const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
      // global ui.language from the EXTENSION config (card lang overrides)
      const [gLang, setGLang] = useState<string | undefined>(undefined)
      useEffect(() => {
        let alive = true
        fetchExtensionUiConfig(extensionId).then((c: { ui?: { language?: string } }) => {
          if (alive) setGLang(c.ui?.language)
        })
        return () => { alive = false }
      }, [extensionId])
      const { t, lang } = useLang(config as Record<string, unknown>, gLang)
      exLabelLang = lang

      useEffect(() => injectStyles(STYLE_ID, STYLES), [])

      const [day, setDay] = useState(() => new Date())
      const [summary, setSummary] = useState<Summary | null>(null)
      const [error, setError] = useState<string | null>(null)
      const [loading, setLoading] = useState(false)
      const mountedRef = useRef(true)
      useEffect(() => {
        mountedRef.current = true
        return () => { mountedRef.current = false }
      }, [])

      const since = useMemo(() => localMidnight(day), [day])

      const refresh = useCallback(async () => {
        setLoading(true)
        const res = await runExtensionCommand<Summary>(
          extensionId, 'get_workout_summary', { day_from: since }
        )
        if (!mountedRef.current) return
        if (res.success && res.data) { setSummary(res.data); setError(null) }
        else setError(res.error || t('loadSummaryFail'))
        setLoading(false)
      }, [extensionId, since])

      useEffect(() => {
        const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
        return () => clearTimeout(t)
      }, [refresh])

      // Live-ish refresh while viewing today; historical days are static.
      const isToday = localMidnight(new Date()) === since
      useEffect(() => {
        if (!isToday) return
        const id = setInterval(() => { if (mountedRef.current) refresh() }, 10000)
        return () => clearInterval(id)
      }, [isToday, refresh])

      const sessions = summary?.sessions ?? []
      const equipment = useMemo(
        () => (summary?.equipment ?? []).sort((a, b) => b.duration_sec - a.duration_sec),
        [summary]
      )

      // ---- Gantt lanes: group sessions by member (named first, then by id) ----
      const lanes = useMemo<MemberLane[]>(() => {
        const map = new Map<string, MemberLane>()
        for (const s of sessions) {
          const key = s.member_id || `anon:${s.id}`
          const name = s.member_name?.trim() || (s.member_id ? s.member_id : t('visitor'))
          let lane = map.get(key)
          if (!lane) {
            lane = { key, name, color: '', total: 0, blocks: [] }
            map.set(key, lane)
          }
          const dur = Math.max(s.duration_sec, 30)
          lane.total += s.duration_sec
          lane.blocks.push({
            id: s.id,
            start: (s.started_at ?? since) - since,
            end: (s.ended_at ?? (s.started_at ?? since) + dur) - since,
            dur: s.duration_sec,
          })
        }
        return [...map.values()]
          .sort((a, b) => b.total - a.total)
          .slice(0, 10)
          .map((l, i) => ({ ...l, color: PALETTE[i % PALETTE.length] }))
      }, [sessions, since])

      // Merge anonymous visitors into one lane when there are too many.
      const lanesMerged = useMemo(() => {
        const anon = lanes.filter((l) => l.key.startsWith('anon:'))
        if (anon.length <= 1) return lanes
        const rest = lanes.filter((l) => !l.key.startsWith('anon:'))
        const total = anon.reduce((s, l) => s + l.total, 0)
        const blocks = anon.flatMap((l) => l.blocks)
        return [
          ...rest,
          { key: 'anon', name: `${t('visitor')} ×${anon.length}`, color: PALETTE[9], total, blocks },
        ]
      }, [lanes])

      const tlMaxSec = useMemo(() => {
        const ends = lanesMerged.flatMap((l) => l.blocks.map((b) => Math.max(b.end, b.start + 60)))
        return Math.max(3600, ...ends)
      }, [lanesMerged])

      // ---- Equipment share bars ----
      const eqTotal = useMemo(
        () => equipment.reduce((s, e) => s + e.duration_sec, 0),
        [equipment]
      )
      const eqRows = useMemo(
        () => equipment.slice(0, 6).map((e, i) => ({
          name: e.exercise ? exLabel(e.exercise) : e.zone_id,
          zone: e.zone_id,
          dur: e.duration_sec,
          reps: e.reps,
          pct: eqTotal > 0 ? Math.round((e.duration_sec / eqTotal) * 100) : 0,
          color: PALETTE[i % PALETTE.length],
        })),
        [equipment, eqTotal]
      )

      const uniqueMembers = new Set(
        sessions.map((s) => s.member_id).filter(Boolean)
      ).size
      const today = localMidnight(new Date())
      const fmtDay = (d: Date) =>
        d.toLocaleDateString([], { month: 'numeric', day: 'numeric' })

      const hourLabel = (sec: number) => {
        const h = Math.floor(sec / 3600)
        return `${String(h).padStart(2, '0')}:00`
      }

      return (
        <div ref={ref} className={`gym-sum ${className}`}>
          <div className="gym-sum-card">
            {/* Header — same rhythm as the other cards (traffic-header) */}
            <div className="gym-traffic-header gym-sum-header">
              <div className="gym-traffic-title gym-sum-title">
                <ClockIcon />
                <span>{t('workoutSummary')}</span>
              </div>
              <div className="gym-traffic-headright gym-sum-daynav">
                <button
                  className="gym-sum-navbtn"
                  onClick={() => setDay(new Date(day.getTime() - DAY_MS))}
                  aria-label={t('prevDay')}
                >‹</button>
                <span className="gym-sum-day">
                  {isToday ? t('today') : fmtDay(day)}
                </span>
                <button
                  className="gym-sum-navbtn"
                  disabled={since >= today}
                  onClick={() => setDay(new Date(day.getTime() + DAY_MS))}
                  aria-label={t('nextDay')}
                >›</button>
              </div>
            </div>
            <div className="gym-sum-body">

            {error && !summary ? (
              <div className="gym-sum-state">
                <span className="gym-sum-state-text">{error}</span>
                <button className="gym-live-retry" onClick={refresh}>{t('retryBtn')}</button>
              </div>
            ) : loading && !summary ? (
              <div className="gym-sum-state">
                <div className="gym-live-spinner" />
                <span className="gym-sum-state-text">{t('loadingDots')}</span>
              </div>
            ) : sessions.length === 0 && equipment.length === 0 ? (
              <div className="gym-sum-state">
                <ClockIcon />
                <span className="gym-sum-state-text">{t('noRecords')}</span>
              </div>
            ) : (
              <>
                {/* Hero stats */}
                <div className="gym-sum-stats">
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">
                      {fmtDuration(summary?.total_duration_sec ?? 0)}
                    </span>
                    <span className="gym-sum-stat-label">{t('totalTime')}</span>
                  </div>
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">
                      {summary?.visit_count ?? 0}
                    </span>
                    <span className="gym-sum-stat-label">{t('visits')}</span>
                  </div>
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">{uniqueMembers}</span>
                    <span className="gym-sum-stat-label">{t('visitedMembers')}</span>
                  </div>
                </div>

                {/* 24h Gantt timeline */}
                {lanesMerged.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">{t('timeline')}</div>
                    <div className="gym-sum-tl">
                      {lanesMerged.map((l) => (
                        <div className="gym-sum-tl-row" key={l.key}>
                          <span className="gym-sum-tl-name" title={l.name}>
                            {l.name}
                          </span>
                          <div className="gym-sum-tl-track">
                            {l.blocks.map((b) => (
                              <div
                                key={b.id}
                                className="gym-sum-tl-block"
                                style={{
                                  left: `${(b.start / tlMaxSec) * 100}%`,
                                  width: `${Math.max(0.8, (Math.max(b.end - b.start, 60) / tlMaxSec) * 100)}%`,
                                  background: l.color,
                                }}
                                title={`${l.name} · ${fmtTime((since + b.start))} - ${fmtTime(since + Math.max(b.end, b.start + 60))} · ${fmtDuration(b.dur)}`}
                              />
                            ))}
                          </div>
                          <span className="gym-sum-tl-dur">
                            {fmtDuration(l.total)}
                          </span>
                        </div>
                      ))}
                      <div className="gym-sum-tl-hours">
                        {[0, 0.25, 0.5, 0.75, 1].map((f) => (
                          <span
                            key={f}
                            style={{ left: `${f * 100}%` }}
                          >{hourLabel(f * tlMaxSec)}</span>
                        ))}
                      </div>
                    </div>
                  </div>
                )}

                {/* Equipment share bars */}
                {eqRows.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">{t('equipmentUsage')}</div>
                    <div className="gym-sum-eqbars">
                      {eqRows.map((e) => (
                        <div className="gym-sum-eqrow" key={e.zone}>
                          <div className="gym-sum-eqhead">
                            <span className="gym-sum-eqname" title={e.zone}>{e.name}</span>
                            <span className="gym-sum-eqval">
                              {fmtDuration(e.dur)}
                              {e.reps > 0 ? ` · ${e.reps}` : ''}
                              <em>{e.pct}%</em>
                            </span>
                          </div>
                          <div className="gym-sum-eqtrack">
                            <div
                              className="gym-sum-eqbar"
                              style={{ width: `${Math.max(2, e.pct)}%`, background: e.color }}
                            />
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* Sessions */}
                {sessions.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">{t('sessionLog')}</div>
                    <div className="gym-sum-sessions">
                      {sessions.slice(0, 8).map((s) => (
                        <div className="gym-sum-session" key={s.id}>
                          <span
                            className="gym-sum-session-dot"
                            aria-hidden="true"
                          />
                          <span className="gym-sum-session-name">
                            {s.member_name || s.member_id || t('visitor')}
                          </span>
                          <span className="gym-sum-session-time">
                            {fmtTime(s.started_at)}
                            {s.ended_at ? `–${fmtTime(s.ended_at)}` : ''}
                          </span>
                          <span className="gym-sum-session-dur">
                            {fmtDuration(s.duration_sec)}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </>
            )}
            </div>
          </div>
        </div>
      )
    }
  )

GymWorkoutSummary.displayName = 'GymWorkoutSummary'
