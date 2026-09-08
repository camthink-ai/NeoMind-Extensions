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
  fetchExtensionUiConfig
} from './common'
import { GymSelect } from './GymSelect'
import { useLang, translator } from './i18n'
import { GymModal } from './GymDrawer'
import STYLES from './styles.css?raw'
import { exIcon } from './ExerciseIcons'

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

interface ExRow {
  exercise: string; sets: number; reps: number; duration_sec: number; sessions: number
}
interface ZoneRow { zone: string; duration_sec: number; reps: number; exercise: string }
interface SessionRow { id?: string; started_at?: number; duration_sec?: number }
interface DailyRow { day: number; exercise: string; sets: number; reps: number; duration_sec: number }
interface Detail {
  days: number; all_history?: boolean; total_duration_sec: number; visits: number
  exercises: ExRow[]
  zones: ZoneRow[]
  sessions: SessionRow[]
  daily?: DailyRow[]
  identity?: { body_samples: number; face_samples: number }
}

/** classifier labels → display names (fallback: the raw label) */
const EX_NAME_EN: Record<string, string> = {
  squat: 'Squat', lunge: 'Lunge', leg_press: 'Leg press', deadlift: 'Deadlift',
  bench_press: 'Bench press', chest_fly: 'Chest fly', pushup: 'Push-up', pullup: 'Pull-up',
  row: 'Row', lat_pulldown: 'Lat pulldown', curl: 'Curl', shoulder_press: 'Shoulder press',
  crunch: 'Crunch', situp: 'Sit-up', plank: 'Plank',
  treadmill: 'Treadmill', run: 'Run', walk: 'Walk', spin_bike: 'Spin bike', cycling: 'Cycling',
  stair_climber: 'Stair climber', jump_rope: 'Jump rope', stretch: 'Stretch',
  standing: 'Standing', unknown: 'Unknown', pending: 'Detecting…',
}
const EX_NAME: Record<string, string> = {
  squat: '深蹲', lunge: '弓步', leg_press: '腿举', deadlift: '硬拉',
  bench_press: '卧推', chest_fly: '夹胸', pushup: '俯卧撑', pullup: '引体向上',
  row: '划船', lat_pulldown: '高位下拉', curl: '弯举', shoulder_press: '推举',
  crunch: '卷腹', situp: '仰卧起坐', plank: '平板支撑',
  treadmill: '跑步机', run: '跑步', walk: '走动', spin_bike: '动感单车', cycling: '骑行',
  stair_climber: '爬楼', jump_rope: '跳绳', stretch: '拉伸',
  standing: '站立体息', unknown: '未识别', pending: '识别中…',
}
const exNameZh = (e: string) => EX_NAME[e] ?? e
const exNameEn = (e: string) => EX_NAME_EN[e] ?? e
/** pick by the ACTIVE product language (en default) */
let exNameLang: 'zh' | 'en' = 'en'
const exName = (e: string) => (exNameLang === 'zh' ? exNameZh(e) : exNameEn(e))
/** CE days (chrono num_days_from_ce) → "M/D 周X" */
const ceDay = (ceDay: number): string => {
  const d = new Date((ceDay - 719163) * 86400000)
  const wd = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][d.getDay()]
  return `${d.getMonth() + 1}/${d.getDate()} ${wd}`
}

/** Lang-aware duration via the shared translator */
type Translator = ReturnType<typeof translator>
const durFmt = (t: Translator, s: number): string =>
  s >= 3600
    ? t('hoursFmt', { v: (s / 3600).toFixed(1) })
    : t('minutesFmt', { v: Math.round(s / 60) })
const fmtTs = (ts?: number) => {
  if (!ts) return '—'
  const d = new Date(ts * 1000)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Expanded per-member panel: records + management (lives in a GymDrawer). */
function MemberDetail({ extensionId, row, mates, days, t, onClose, onChanged }: {
  extensionId: string
  row: ReportRow
  mates: ReportRow[]
  days: number
  t: ReturnType<typeof translator>
  onClose: () => void
  onChanged: () => void
}) {
  const [det, setDet] = useState<Detail | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [name, setName] = useState(row.name)
  const [merging, setMerging] = useState(false)
  // detail window: the card's `days` or the entire history
  const [allHist, setAllHist] = useState(false)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  const load = useCallback(async () => {
    const r = await runExtensionCommand<Detail>(extensionId, 'get_member_workout_detail', {
      member_id: row.member_id, days: allHist ? 0 : days,
    })
    if (!mounted.current) return
    if (r.success && r.data) { setDet(r.data); setErr(null) }
    else setErr(r.error || t('loadFailed'))
  }, [extensionId, row.member_id, days, allHist])

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
    if (!window.confirm(t('deleteConfirm', { name: row.name }))) return
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
          placeholder={t('memberNamePh')}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') doRename() }}
        />
        <button className="gym-ov-btn" disabled={busy || !name.trim() || name.trim() === row.name}
          onClick={doRename} title={t('enterToSave')}>{t('rename')}</button>
        {merging ? (
          <GymSelect
            value=""
            autoOpen
            placeholder={t('mergeIntoPh')}
            onClose={() => setMerging(false)}
            onChange={doMerge}
            options={mates
              .filter((m) => m.member_id !== row.member_id)
              .map((m) => ({ value: m.member_id, label: m.name }))}
          />
        ) : (
          <button className="gym-ov-btn" disabled={busy || mates.length < 2}
            onClick={() => setMerging(true)}
            title={t('mergeTip')}>{t('merge')}</button>
        )}
        <button className="gym-ov-btn danger" disabled={busy} onClick={doDelete}>{t('del')}</button>
        <span className="gym-detail-window">
          <button className={`gym-ov-tg ${!allHist ? 'on' : ''}`}
            onClick={() => setAllHist(false)}>{t('lastNDays', { n: days })}</button>
          <button className={`gym-ov-tg ${allHist ? 'on' : ''}`}
            onClick={() => setAllHist(true)}>{t('allHistory')}</button>
        </span>
      </div>

      {err && <div className="gym-report-records-error">{err}</div>}
      {!det && !err && <div className="gym-report-records-empty">{t('recordsLoading')}</div>}
      {det && (
        <div className="gym-report-records">
          {/* identity samples: how this member is recognized */}
          {det.identity && (
            <div className="gym-identity-strip">
              <span className="gym-identity-item">
                <b>{det.identity.body_samples}</b> {t('bodyReidSamples')}
              </span>
              <span className="gym-identity-item">
                <b>{det.identity.face_samples}</b> {t('faceSamples')}
              </span>
              <span className="gym-identity-item dim">
                {det.identity.face_samples > 0
                  ? t('dualChannel')
                  : t('reidOnly')}
              </span>
            </div>
          )}
          {/* hero stats */}
          <div className="gym-detail-stats">
            <div className="gym-detail-stat">
              <span className="gym-detail-stat-v">{durFmt(t, det.total_duration_sec)}</span>
              <span className="gym-detail-stat-k">{t('totalDuration')}</span>
            </div>
            <div className="gym-detail-stat">
              <span className="gym-detail-stat-v">{det.visits}</span>
              <span className="gym-detail-stat-k">{t('visitTimes')}</span>
            </div>
            <div className="gym-detail-stat">
              <span className="gym-detail-stat-v">{det.exercises.length}</span>
              <span className="gym-detail-stat-k">{t('trainedMoves')}</span>
            </div>
          </div>

          {/* {t('motionAnalysis')} */}
          <div className="gym-detail-sec">
            <span className="gym-detail-sec-title">
              {t('motionAnalysis')} <em>{allHist ? t('allHistory') : t('lastNDays', { n: days })}</em>
            </span>
            {det.exercises.length > 0 ? (
              <div className="gym-report-exlist">
                {det.exercises.map((e) => {
                  const maxSec = Math.max(1, ...det.exercises.map((x) => x.duration_sec))
                  const ExIcon = exIcon(e.exercise)
                  return (
                    <div key={e.exercise} className="gym-report-exrow">
                      <span className="gym-report-exicon"><ExIcon /></span>
                      <span className="gym-report-exname">{exName(e.exercise)}</span>
                      <div className="gym-report-exbar">
                        <div className="gym-report-exbar-fill" style={{ width: `${Math.max(3, (e.duration_sec / maxSec) * 100)}%` }} />
                      </div>
                      <span className="gym-report-exmeta">
                        {e.reps > 0 && t('setsReps', { s: e.sets, r: e.reps })}
                        {e.reps > 0 ? ' · ' : ''}{durFmt(t, e.duration_sec)}
                      </span>
                    </div>
                  )
                })}
              </div>
            ) : (
              <div className="gym-report-records-empty">
                {t('noMotionData')}
              </div>
            )}
          </div>

          {/* {t('equipDist')} */}
          {det.zones.length > 0 && (
            <div className="gym-detail-sec">
              <span className="gym-detail-sec-title">{t('equipDist')}</span>
              <div className="gym-report-eq">
                {det.zones.map((z) => (
                  <div key={z.zone} className="gym-report-eq-row">
                    <span className="gym-report-eq-name">{z.zone}</span>
                    <span className="gym-report-eq-meta">
                      {durFmt(t, z.duration_sec)}{z.reps > 0 ? ` · ${t('timesPlain', { n: z.reps })}` : ''}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* 健身历史（按天，全部历史模式） */}
          {allHist && (det.daily?.length ?? 0) > 0 && (
            <div className="gym-detail-sec">
              <span className="gym-detail-sec-title">
                {t('fitnessHistory')} <em>{t('byDay')} · {new Set(det.daily!.map((d) => d.day)).size} {t('daysTrained')}</em>
              </span>
              {(() => {
                const byDay = new Map<number, DailyRow[]>()
                for (const d of det.daily!) {
                  if (!byDay.has(d.day)) byDay.set(d.day, [])
                  byDay.get(d.day)!.push(d)
                }
                return [...byDay.entries()].map(([day, rows]) => {
                  const secs = rows.reduce((s, r) => s + r.duration_sec, 0)
                  const reps = rows.reduce((s, r) => s + r.reps, 0)
                  return (
                    <div key={day} className="gym-history-day">
                      <div className="gym-history-day-head">
                        <span className="gym-history-day-date">{ceDay(day)}</span>
                        <span className="gym-history-day-sum">
                          {t('historySum', { m: rows.length })}{reps > 0 ? ` · ${t('timesShort', { n: reps })}` : ''} · {durFmt(t, secs)}
                        </span>
                      </div>
                      {rows.map((r, i) => {
                        const HIcon = exIcon(r.exercise)
                        return (
                          <div key={i} className="gym-history-line">
                            <span className="gym-history-ex">
                              <span className="gym-report-exicon"><HIcon /></span>
                              {exName(r.exercise)}
                            </span>
                            <span className="gym-history-meta">
                              {r.reps > 0 ? t('setsReps', { s: r.sets, r: r.reps }) + ' · ' : ''}{durFmt(t, r.duration_sec)}
                            </span>
                          </div>
                        )
                      })}
                    </div>
                  )
                })
              })()}
            </div>
          )}

          {/* {t('visitLog')} */}
          <div className="gym-detail-sec">
            <span className="gym-detail-sec-title">{t('visitLog')}</span>
            {det.sessions.length > 0 ? (
              <div className="gym-report-sessions">
                {det.sessions.slice(0, 8).map((s) => (
                  <div key={s.id ?? s.started_at} className="gym-report-session">
                    <span className="gym-report-session-when">{fmtTs(s.started_at)}</span>
                    <span className="gym-report-session-dur">{durFmt(t, s.duration_sec ?? 0)}</span>
                  </div>
                ))}
              </div>
            ) : (
              <div className="gym-report-records-empty">
                {t('noVisitData', { n: days })}
              </div>
            )}
          </div>
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
    exNameLang = lang

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
      else if (!r.success) setError(r.error || t('loadFailed'))
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
            <span className="gym-ov-title">{t('memberVisits')} · {t('lastNDays', { n: days })}</span>
            <span className="gym-ov-badge">
              {rep ? `${rep.total_visits} / ${rep.active_members}` : '…'}
            </span>
          </div>
          <div className="gym-report-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            {!error && rep && rep.rows.length === 0 && (
              <div className="gym-rank-empty">
                {t('noMemberRecords')}
              </div>
            )}
            {rep && rep.rows.length > 0 && (
              <div className="gym-report-grid">
                {rep.rows.slice(0, 10).map((m) => {
                  const src = memberPhotoSrc(m.photo)
                  const open = openId === m.member_id
                  return (
                    <div
                      key={m.member_id}
                      className={`gym-mcard clickable ${open ? 'sel' : ''}`}
                      onClick={() => setOpenId(open ? null : m.member_id)}
                      title={open ? undefined : t('mergeTip')}
                    >
                      <div className="gym-mcard-head">
                        {src ? (
                          <img className="gym-report-avatar" src={src} alt={m.name} title={m.name} />
                        ) : (
                          <span className="gym-report-avatar gym-report-avatar-fb" title={m.name}>
                            {(m.name || '?').slice(0, 1)}
                          </span>
                        )}
                        <span className="gym-mcard-name">
                          {m.name}
                          {m.source === 'auto' && <span className="gym-report-vtag">{t('visitor')}</span>}
                        </span>
                      </div>
                      <div className="gym-mcard-stats">
                        <span className="gym-mcard-stat">
                          <span className="gym-mcard-stat-v accent">{m.visits}</span>
                          <span className="gym-mcard-stat-k">{t('visitsShort')}</span>
                        </span>
                        <span className="gym-mcard-stat">
                          <span className="gym-mcard-stat-v">{durFmt(t, m.duration_sec)}</span>
                          <span className="gym-mcard-stat-k">{t('durationShort')}</span>
                        </span>
                        <span className="gym-mcard-stat">
                          <span className="gym-mcard-stat-v dim">{fmtTs(m.last_seen)}</span>
                          <span className="gym-mcard-stat-k">{t('lastShort')}</span>
                        </span>
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        </div>
        {rep && openId && (() => {
          const m = rep.rows.find((r) => r.member_id === openId)
          if (!m) return null
          const src = memberPhotoSrc(m.photo)
          return (
            <GymModal open onClose={() => setOpenId(null)} width={920}
              title={
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                  {src && <img className="gym-report-avatar" src={src} alt={m.name} />}
                  {m.name}
                  {m.source === 'auto' && <span className="gym-report-vtag">{t('visitor')}</span>}
                </span>
              }>
              <MemberDetail
                extensionId={extensionId}
                t={t}
                row={m}
                mates={rep.rows.slice(0, 10)}
                days={days}
                onClose={() => setOpenId(null)}
                onChanged={refresh}
              />
            </GymModal>
          )
        })()}
      </div>
    )
  },
)
