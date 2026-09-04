/**
 * GymMemberReport — member visit report over the last N days.
 *
 * get_member_report aggregates sessions by member: visits / minutes /
 * last-seen, plus library totals. Empty until members are registered
 * (walk-in anonymous sessions deliberately don't rank).
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  injectStyles,
  runExtensionCommand,
} from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-report-styles-v1'

interface ReportRow { member_id: string; name: string; visits: number; duration_sec: number; last_seen: number }
interface Report { days: number; members_total: number; active_members: number; total_visits: number; rows: ReportRow[] }

export const GymMemberReport = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymMemberReport(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const days = Math.min(90, Math.max(1, Number(config?.days) || 7))

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [rep, setRep] = useState<Report | null>(null)
    const [error, setError] = useState<string | null>(null)
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

    const fmtDur = (s: number) =>
      s >= 3600 ? `${(s / 3600).toFixed(1)} 小时` : `${Math.round(s / 60)} 分钟`
    const fmtSeen = (ts: number) => {
      if (!ts) return '—'
      const d = new Date(ts * 1000)
      return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
    }

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
            {rep && rep.rows.slice(0, 10).map((m) => (
              <div className="gym-report-row" key={m.member_id}>
                <span className="gym-report-name">{m.name}</span>
                <span className="gym-report-visits">{m.visits} 次</span>
                <span className="gym-report-dur">{fmtDur(m.duration_sec)}</span>
                <span className="gym-report-seen">最近 {fmtSeen(m.last_seen)}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    )
  },
)
