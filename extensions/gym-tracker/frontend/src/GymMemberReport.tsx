/**
 * GymMemberReport — member visit report over the last N days, and the
 * member-management surface: click a row to expand that member's fitness
 * records (sessions, per-equipment time/reps) plus rename / merge / delete.
 *
 * get_member_report aggregates sessions by member: visits / minutes /
 * last-seen, plus library totals. The detail panel rides get_workout_summary
 * (member_id + day_from) — the same aggregate the Monitor's summary card
 * uses, scoped to one person.
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  deleteMember,
  injectStyles,
  memberPhotoSrc,
  mergeMembers,
  renameMember,
  runExtensionCommand,
} from './common'
import { GymSelect } from './GymSelect'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-report-styles-v1'

interface ReportRow {
  member_id: string; name: string; visits: number; duration_sec: number; last_seen: number
  /** base64 JPEG avatar (server caps photos to the head of the list) */
  photo?: string | null
  /** "auto" = auto-enrolled walk-in, "manual" = registered member */
  source?: string
}
interface Report {
  days: number; members_total: number; active_members: number; total_visits: number; rows: ReportRow[]
}

interface Summary {
  sessions: Array<{
    id?: string; member_name?: string; started_at?: number
    duration_sec?: number; summary?: string
  }>
  equipment: Array<{ zone_id: string; duration_sec: number; reps: number; exercise: string }>
  total_duration_sec: number
}

const fmtDur = (s: number) =>
  s >= 3600 ? `${(s / 3600).toFixed(1)} 小时` : `${Math.round(s / 60)} 分钟`
const fmtTs = (ts?: number) => {
  if (!ts) return '—'
  const d = new Date(ts * 1000)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Expanded per-member panel: records + management. */
function MemberDetail({ extensionId, row, mates, days, onClose, onChanged }: {
  extensionId: string
  row: ReportRow
  mates: ReportRow[]
  days: number
  onClose: () => void
  onChanged: () => void
}) {
  const [sum, setSum] = useState<Summary | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [name, setName] = useState(row.name)
  const [merging, setMerging] = useState(false)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  const load = useCallback(async () => {
    const dayFrom = Math.floor(Date.now() / 1000) - days * 86400
    const r = await runExtensionCommand<Summary>(extensionId, 'get_workout_summary', {
      member_id: row.member_id, day_from: dayFrom,
    })
    if (!mounted.current) return
    if (r.success && r.data) { setSum(r.data); setErr(null) }
    else setErr(r.error || '记录加载失败')
  }, [extensionId, row.member_id, days])

  useEffect(() => { load() }, [load])

  const doRename = async () => {
    const v = name.trim()
    if (!v || v === row.name || busy) return
    setBusy(true)
    const r = await renameMember(extensionId, row.member_id, v)
    setBusy(false)
    if (r.success) onChanged()
  }
  const doMerge = async (targetId: string) => {
    if (busy) return
    setBusy(true)
    const r = await mergeMembers(extensionId, row.member_id, targetId)
    setBusy(false)
    setMerging(false)
    if (r.success) { onChanged(); onClose() }
  }
  const doDelete = async () => {
    if (busy) return
    if (!window.confirm(`删除会员「${row.name}」？其特征与到店历史将一并移除。`)) return
    setBusy(true)
    const r = await deleteMember(extensionId, row.member_id)
    setBusy(false)
    if (r.success) { onChanged(); onClose() }
  }

  return (
    <div className="gym-report-detail">
      <div className="gym-report-detail-manage">
        <input
          className="gym-ov-input name"
          value={name}
          placeholder="会员姓名"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') doRename() }}
        />
        <button className="gym-ov-btn" disabled={busy || !name.trim() || name.trim() === row.name}
          onClick={doRename} title="回车也可保存">改名</button>
        {merging ? (
          <GymSelect
            value=""
            autoOpen
            placeholder="并入哪位会员？"
            onClose={() => setMerging(false)}
            onChange={doMerge}
            options={mates
              .filter((m) => m.member_id !== row.member_id)
              .map((m) => ({ value: m.member_id, label: m.name }))}
          />
        ) : (
          <button className="gym-ov-btn" disabled={busy || mates.length < 2}
            onClick={() => setMerging(true)}
            title="把此人的到店与特征并入另一位（同一人换装/重复录入时用）">并入</button>
        )}
        <button className="gym-ov-btn danger" disabled={busy} onClick={doDelete}>删除</button>
        <span className="gym-report-detail-flex" />
        <button className="gym-ov-btn" onClick={onClose}>收起</button>
      </div>

      {err && <div className="gym-report-records-error">{err}</div>}
      {!sum && !err && <div className="gym-report-records-empty">记录加载中…</div>}
      {sum && (
        <div className="gym-report-records">
          <div className="gym-report-records-head">
            近{days}天：{fmtDur(sum.total_duration_sec)} · {sum.sessions.length} 次训练
            {row.source === 'auto' && <span className="gym-report-vtag">自动录入</span>}
          </div>
          {sum.equipment.length > 0 && (
            <div className="gym-report-eq">
              {sum.equipment.map((e) => (
                <div key={e.zone_id} className="gym-report-eq-row">
                  <span className="gym-report-eq-name">{e.zone_id}</span>
                  <span className="gym-report-eq-meta">
                    {e.exercise || '—'} · {fmtDur(e.duration_sec)}
                    {e.reps > 0 ? ` · ${e.reps} 次` : ''}
                  </span>
                </div>
              ))}
            </div>
          )}
          {sum.sessions.length > 0 && (
            <div className="gym-report-sessions">
              {sum.sessions.slice(0, 8).map((s, i) => (
                <div key={s.id ?? i} className="gym-report-session">
                  <span className="gym-report-session-when">{fmtTs(s.started_at)}</span>
                  <span className="gym-report-session-dur">{fmtDur(s.duration_sec ?? 0)}</span>
                </div>
              ))}
            </div>
          )}
          {sum.sessions.length === 0 && (
            <div className="gym-report-records-empty">
              近{days}天没有训练记录（到店即产生一次训练）
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export const GymMemberReport = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymMemberReport(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const days = Math.min(90, Math.max(1, Number(config?.days) || 7))

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [rep, setRep] = useState<Report | null>(null)
    const [error, setError] = useState<string | null>(null)
    const [openId, setOpenId] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const r = await runExtensionCommand<Report>(extensionId, 'get_member_report', { days })
      if (!mountedRef.current) return
      if (r.success && r.data) setRep(r.data)
      else if (!r.success) setError(r.error || '加载失败')
    }, [extensionId, days])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])
    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 30000)
      return () => clearInterval(id)
    }, [refresh])

    return (
      <div ref={ref} className={`gym-report ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <span className="gym-ov-title">会员到店 · 近{days}天</span>
            <span className="gym-ov-badge">
              {rep ? `${rep.total_visits} 人次 / ${rep.active_members} 位会员` : '…'}
            </span>
          </div>
          <div className="gym-report-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            {!error && rep && rep.rows.length === 0 && (
              <div className="gym-rank-empty">
                还没有会员到店记录——注册会员后自动累计（匿名访客不计入）
              </div>
            )}
            {rep && rep.rows.slice(0, 10).map((m) => {
              const src = memberPhotoSrc(m.photo)
              const open = openId === m.member_id
              return (
                <div key={m.member_id} className="gym-report-rowwrap">
                  <div
                    className={`gym-report-row clickable ${open ? 'sel' : ''}`}
                    onClick={() => setOpenId(open ? null : m.member_id)}
                    title={open ? undefined : '点击展开健身记录与管理'}
                  >
                    {src ? (
                      <img className="gym-report-avatar" src={src} alt={m.name} title={m.name} />
                    ) : (
                      <span className="gym-report-avatar gym-report-avatar-fb" title={m.name}>
                        {(m.name || '?').slice(0, 1)}
                      </span>
                    )}
                    <span className="gym-report-name">{m.name}</span>
                    {m.source === 'auto' && <span className="gym-report-vtag">访客</span>}
                    <span className="gym-report-visits">{m.visits} 次</span>
                    <span className="gym-report-dur">{fmtDur(m.duration_sec)}</span>
                    <span className="gym-report-seen">最近 {fmtTs(m.last_seen)}</span>
                  </div>
                  {open && (
                    <MemberDetail
                      extensionId={extensionId}
                      row={m}
                      mates={rep.rows.slice(0, 10)}
                      days={days}
                      onClose={() => setOpenId(null)}
                      onChanged={refresh}
                    />
                  )}
                </div>
              )
            })}
          </div>
        </div>
      </div>
    )
  },
)
