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
} from './common'
import STYLES from './styles.css?raw'

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
      const equipment = useMemo(
        () => (summary?.equipment ?? []).sort((a, b) => b.duration_sec - a.duration_sec),
        [summary]
      )

      // ---- Gantt lanes: group sessions by member (named first, then by id) ----
      const lanes = useMemo<MemberLane[]>(() => {
        const map = new Map<string, MemberLane>()
        for (const s of sessions) {
          const key = s.member_id || `anon:${s.id}`
          const name = s.member_name?.trim() || (s.member_id ? s.member_id : '访客')
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
          { key: 'anon', name: `访客 ×${anon.length}`, color: PALETTE[9], total, blocks },
        ]
      }, [lanes])

      const tlMaxSec = useMemo(() => {
        const ends = lanesMerged.flatMap((l) => l.blocks.map((b) => Math.max(b.end, b.start + 60)))
        return Math.max(3600, ...ends)
      }, [lanesMerged])

      // ---- Donut geometry ----
      const donut = useMemo(() => {
        const total = equipment.reduce((s, e) => s + e.duration_sec, 0)
        if (total <= 0) return null
        const R = 15.9155 // circumference 100 → stroke-dasharray in %
        const C = 2 * Math.PI * R
        let acc = 0
        const segs = equipment.slice(0, 6).map((e, i) => {
          const frac = e.duration_sec / total
          const seg = {
            color: PALETTE[i % PALETTE.length],
            dash: frac * C,
            offset: -acc * C,
            name: e.exercise ? exLabel(e.exercise) : e.zone_id,
            dur: e.duration_sec,
            pct: Math.round(frac * 100),
          }
          acc += frac
          return seg
        })
        return { segs, total, C, R }
      }, [equipment])

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

                {/* 24h Gantt timeline */}
                {lanesMerged.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">到店时间轴</div>
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

                {/* Equipment donut + legend */}
                {donut && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">器材使用时长</div>
                    <div className="gym-sum-donutrow">
                      <svg
                        className="gym-sum-donut"
                        viewBox="0 0 40 40"
                        role="img"
                        aria-label="器材使用时长占比"
                      >
                        <circle
                          className="gym-sum-donut-bg"
                          cx="20" cy="20" r={donut.R}
                          fill="none" strokeWidth="6"
                        />
                        {donut.segs.map((s, i) => (
                          <circle
                            key={i}
                            cx="20" cy="20" r={donut.R}
                            fill="none"
                            stroke={s.color}
                            strokeWidth="6"
                            strokeDasharray={`${s.dash} ${donut.C - s.dash}`}
                            strokeDashoffset={s.offset}
                            transform="rotate(-90 20 20)"
                          >
                            <title>{`${s.name} ${fmtDuration(s.dur)} (${s.pct}%)`}</title>
                          </circle>
                        ))}
                        <text
                          className="gym-sum-donut-total"
                          x="20" y="19" textAnchor="middle"
                        >{fmtDuration(donut.total)}</text>
                        <text
                          className="gym-sum-donut-sub"
                          x="20" y="25" textAnchor="middle"
                        >总计</text>
                      </svg>
                      <div className="gym-sum-legend">
                        {donut.segs.map((s, i) => (
                          <div className="gym-sum-legrow" key={i}>
                            <span
                              className="gym-sum-legdot"
                              style={{ background: s.color }}
                            />
                            <span className="gym-sum-legname" title={s.name}>
                              {s.name}
                            </span>
                            <span className="gym-sum-legval">
                              {fmtDuration(s.dur)} · {s.pct}%
                            </span>
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                )}

                {/* Sessions */}
                {sessions.length > 0 && (
                  <div className="gym-sum-section">
                    <div className="gym-sum-section-title">训练记录</div>
                    <div className="gym-sum-sessions">
                      {sessions.slice(0, 8).map((s) => (
                        <div className="gym-sum-session" key={s.id}>
                          <span
                            className="gym-sum-session-dot"
                            aria-hidden="true"
                          />
                          <span className="gym-sum-session-name">
                            {s.member_name || s.member_id || '访客'}
                          </span>
                          <span className="gym-sum-session-time">
                            {fmtTime(s.started_at)} 入场
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
