/**
 * GymWorkoutSummary — daily workout analytics card.
 *
 * Reads `get_workout_summary` (sessions + equipment_usage since local
 * midnight, or the picked day): total training time, visit count, per-zone
 * equipment duration ranking, and the session list with member names.
 * Day navigation is client-side — `day_from` is recomputed as the local
 * midnight of the picked date.
 */

import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  runExtensionCommand,
  injectStyles,
} from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-sum-styles-v1'

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

const EXERCISE_ZH: Record<string, string> = {
  treadmill_run: '跑步机',
  walk: '步行',
  run: '跑步',
  squat: '深蹲',
  lunge: '弓步',
  deadlift: '硬拉',
  bench: '卧推',
  pushup: '俯卧撑',
  chest_fly: '飞鸟',
  crunch: '卷腹',
  situp: '仰卧起坐',
  plank: '平板支撑',
  pullup: '引体向上',
  shoulder_press: '肩推',
  bicep_curl: '弯举',
  lateral_raise: '侧平举',
  dumbbell_row: '哑铃划船',
  cycling: '动感单车',
  rowing: '划船机',
  elliptical: '椭圆机',
  standing: '站立',
}

const exLabel = (name: string): string => EXERCISE_ZH[name] ?? name

const ClockIcon = ({ className = '' }: { className?: string }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor"
    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="10" />
    <polyline points="12 6 12 12 16 14" />
  </svg>
)

const DAY_MS = 86400000

export const GymWorkoutSummary =
  forwardRef<HTMLDivElement, ExtensionComponentProps>(
    function GymWorkoutSummary(props, ref) {
      const { dataSource, className = '' } = props
      const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID

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
        else setError(res.error || '加载概况失败')
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
      const equipment = summary?.equipment ?? []
      const maxEqSec = Math.max(1, ...equipment.map((e) => e.duration_sec))
      const uniqueMembers = new Set(
        sessions.map((s) => s.member_id).filter(Boolean)
      ).size
      const today = localMidnight(new Date())
      const fmtDay = (d: Date) =>
        d.toLocaleDateString([], { month: 'numeric', day: 'numeric' })

      return (
        <div ref={ref} className={`gym-sum ${className}`}>
          <div className="gym-sum-card">
            {/* Header */}
            <div className="gym-sum-header">
              <div className="gym-sum-title">
                <ClockIcon />
                <span>运动概况</span>
              </div>
              <div className="gym-sum-daynav">
                <button
                  className="gym-sum-navbtn"
                  onClick={() => setDay(new Date(day.getTime() - DAY_MS))}
                  aria-label="前一天"
                >‹</button>
                <span className="gym-sum-day">
                  {isToday ? '今天' : fmtDay(day)}
                </span>
                <button
                  className="gym-sum-navbtn"
                  disabled={since >= today}
                  onClick={() => setDay(new Date(day.getTime() + DAY_MS))}
                  aria-label="后一天"
                >›</button>
              </div>
            </div>

            {error && !summary ? (
              <div className="gym-sum-state">
                <span className="gym-sum-state-text">{error}</span>
                <button className="gym-live-retry" onClick={refresh}>重试</button>
              </div>
            ) : loading && !summary ? (
              <div className="gym-sum-state">
                <div className="gym-live-spinner" />
                <span className="gym-sum-state-text">加载中…</span>
              </div>
            ) : sessions.length === 0 && equipment.length === 0 ? (
              <div className="gym-sum-state">
                <ClockIcon />
                <span className="gym-sum-state-text">当日暂无运动记录</span>
              </div>
            ) : (
              <>
                {/* Hero stats */}
                <div className="gym-sum-stats">
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">
                      {fmtDuration(summary?.total_duration_sec ?? 0)}
                    </span>
                    <span className="gym-sum-stat-label">总时长</span>
                  </div>
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">
                      {summary?.visit_count ?? 0}
                    </span>
                    <span className="gym-sum-stat-label">训练场次</span>
                  </div>
                  <div className="gym-sum-stat">
                    <span className="gym-sum-stat-value">{uniqueMembers}</span>
                    <span className="gym-sum-stat-label">到访会员</span>
                  </div>
                </div>

                {/* Equipment ranking */}
                {equipment.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">器材使用时长</div>
                    <div className="gym-sum-eqlist">
                      {equipment.map((e) => (
                        <div className="gym-sum-eqrow" key={e.zone_id}>
                          <span className="gym-sum-eqname" title={e.zone_id}>
                            {e.exercise ? exLabel(e.exercise) : e.zone_id}
                          </span>
                          <div className="gym-sum-eqbar">
                            <div
                              className="gym-sum-eqfill"
                              style={{
                                width: `${Math.max(4, (e.duration_sec / maxEqSec) * 100)}%`,
                              }}
                            />
                          </div>
                          <span className="gym-sum-eqsec">
                            {fmtDuration(e.duration_sec)}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* Sessions */}
                {sessions.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">训练记录</div>
                    <div className="gym-sum-sessions">
                      {sessions.map((s) => (
                        <div className="gym-sum-session" key={s.id}>
                          <span
                            className="gym-sum-session-dot"
                            aria-hidden="true"
                          />
                          <span className="gym-sum-session-name">
                            {s.member_name || s.member_id || '访客'}
                          </span>
                          <span className="gym-sum-session-time">
                            {fmtTime(s.started_at)}
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
      )
    }
  )

GymWorkoutSummary.displayName = 'GymWorkoutSummary'
