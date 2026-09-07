/**
 * i18n — tiny dictionary for the gym-tracker widgets. Every card accepts a
 * `lang` config ('zh' default | 'en'); `useLang(config)` returns a `t()`
 * bound to that language. Keys missing from a dict fall back to the key's
 * zh value, so partially-translated surfaces stay functional.
 */
import { useMemo } from 'react'
import type { ExtensionComponentProps } from './common'

export type Lang = 'zh' | 'en'

const zh = {
  // generic
  loading: '加载中…', loadFailed: '加载失败', retry: '重试',
  live: '实时', replay: '回看', close: '关闭', save: '保存', done: '完成',
  cancel: '取消', undo: '撤销', list: '列表', hideList: '隐藏列表',
  today: '今天', empty: '暂无数据',
  // Monitor
  peopleInGym: '人在场', frames: '框', skeleton: '骨架', zones: '分区',
  mosaic: '打码', edit: '编辑', zoneManage: '分区管理', lineManage: '计数线管理',
  excludeManage: '无效区管理', closePoly: '闭合',
  // Equipment
  equipment: '器械', busy: '占用', onGear: '使用中', idle: '空闲',
  people: '人', visitor: '访客',
  // Traffic
  trafficTitle: '人流趋势', lastHours: '近{n}小时',
  // Summary
  workoutSummary: '运动概况', totalTime: '总时长', visits: '训练场次',
  visitedMembers: '到访会员', timeline: '到店时间轴', equipmentUsage: '器材使用时长',
  sessionLog: '训练记录', noRecords: '当日暂无运动记录',
  // Rank
  equipRank: '器械使用排行', noUsageToday: '今日还没有器械使用记录',
  units: '台',
  // Alerts
  alertsTitle: '实时告警', allGood: '一切正常',
  noAlerts: '暂无告警——跌倒检测与器械久占监控运行中',
  // Member report
  memberVisits: '会员到店', member: '会员', lastSeen: '最近',
  duration: '时长', totalDuration: '总时长', visitCount: '到店次数',
  trainedMoves: '训练动作', motionAnalysis: '动作分析', allHistory: '全部历史',
  lastNDays: '近{n}天', equipDist: '器械分布', visitLog: '到店记录',
  bodyReidSamples: '人体 ReID 样本', faceSamples: '人脸样本',
  dualChannel: '人脸识别 + 人体 ReID 双通道', reidOnly: '人体 ReID 识别中（人脸样本待采集）',
  rename: '改名', merge: '并入', del: '删除',
  visitTimes: '到店次数', setsReps: '{s}组·{r}次', timesShort: '{n} 次',
  timesPlain: '{n} 次', historySum: '{m} 项动作',
  noVisitLog: '近{n}天没有到店记录', noMemberRecords: '还没有会员到店记录——注册会员后自动累计（匿名访客不计入）',
  deleteConfirm: '删除会员「{name}」？其特征与到店历史将一并移除。',
  mergeTip: '把此人的到店与特征并入另一位（同一人换装/重复录入时用）',
  visitsShort: '到店', durationShort: '时长', lastShort: '最近',
  autoEnrolled: '自动录入', noVisitData: '近{n}天没有到店记录',
  fitnessHistory: '健身历史', byDay: '按天', daysTrained: '天有训练',
  movesCount: '项动作', noMotionData: '暂无动作识别数据——会员在 mapped 器械区训练后自动累积',
  // Door flow
  doorTitle: '进出场', todayIn: '今日进场', todayOut: '今日出场',
  netIn: '净在场', in: '进', out: '出', date: '日期',
  noDoorRecords: '还没有进出场记录——画好计数线后，过线即开始累计并按天留档',
  // Trails / Heat cards
  trailsTitle: '轨迹', heatTitle: '热力', todayPeak: '今日峰值',
  low: '低', high: '高', samples: '样本',
  noFootprint: '该时段没有足迹记录',
  footprintNote: '足迹日志自启用起累积；拖回最右侧查看实时',
  replaySuffix: '回看',
}

type Dict = typeof zh

const en: Partial<Dict> = {
  loading: 'Loading…', loadFailed: 'Load failed', retry: 'Retry',
  live: 'Live', replay: 'Replay', close: 'Close', save: 'Save', done: 'Done',
  cancel: 'Cancel', undo: 'Undo', list: 'List', hideList: 'Hide list',
  today: 'Today', empty: 'No data',
  peopleInGym: 'in gym', frames: 'Boxes', skeleton: 'Skeleton', zones: 'Zones',
  mosaic: 'Mosaic', edit: 'Edit', zoneManage: 'Zones', lineManage: 'Count lines',
  excludeManage: 'Exclusions', closePoly: 'Close',
  equipment: 'Equipment', busy: 'busy', onGear: 'on gear', idle: 'Idle',
  people: 'ppl', visitor: 'Guest',
  trafficTitle: 'Traffic', lastHours: 'last {n}h',
  workoutSummary: 'Workout Summary', totalTime: 'Total time', visits: 'Sessions',
  visitedMembers: 'Members', timeline: 'Presence timeline', equipmentUsage: 'Equipment usage',
  sessionLog: 'Sessions', noRecords: 'No workouts recorded today',
  equipRank: 'Equipment Rank', noUsageToday: 'No equipment usage today',
  units: 'units',
  alertsTitle: 'Alerts', allGood: 'All clear',
  noAlerts: 'No alerts — fall detection & occupancy watch running',
  memberVisits: 'Member Visits', member: 'Member', lastSeen: 'Last',
  duration: 'Time', totalDuration: 'Total time', visitCount: 'Visits',
  trainedMoves: 'Exercises', motionAnalysis: 'Exercise analysis', allHistory: 'All history',
  lastNDays: 'last {n}d', equipDist: 'Equipment split', visitLog: 'Visits',
  bodyReidSamples: 'Body ReID samples', faceSamples: 'Face samples',
  dualChannel: 'Face + body ReID dual channel', reidOnly: 'Body ReID only (face pending)',
  rename: 'Rename', merge: 'Merge', del: 'Delete',
  visitTimes: 'Visits', setsReps: '{s}×{r}',
  timesShort: '{n} reps', timesPlain: '{n}',
  historySum: '{m} moves',
  noVisitLog: 'No visits in the last {n} days',
  noMemberRecords: 'No member visits yet — register members to start tracking (anonymous walk-ins excluded)',
  deleteConfirm: 'Delete member "{name}"? Their identity samples and visit history will be removed.',
  mergeTip: 'Merge this person into another member (same person, different outfit)',
  visitsShort: 'Visits', durationShort: 'Time', lastShort: 'Last',
  autoEnrolled: 'Auto-enrolled', noVisitData: 'No visits in the last {n} days',
  fitnessHistory: 'Workout history', byDay: 'by day', daysTrained: 'days with training',
  movesCount: 'exercises', noMotionData: 'No exercise data yet — accumulates once the member trains on mapped zones',
  doorTitle: 'Door Flow', todayIn: 'In today', todayOut: 'Out today',
  netIn: 'Net inside', in: 'In', out: 'Out', date: 'Date',
  noDoorRecords: 'No crossings yet — draw a counting line and traffic starts logging daily',
  trailsTitle: 'Trails', heatTitle: 'Heatmap', todayPeak: 'Peak today',
  low: 'Low', high: 'High', samples: 'samples',
  noFootprint: 'No footprints in this window',
  footprintNote: 'Footprint log accumulates from activation; drag right for live',
  replaySuffix: ' replay',
}

const DICTS: Record<Lang, Partial<Dict>> = { zh, en }

export function translator(lang: Lang) {
  const dict = DICTS[lang] ?? zh
  return (key: keyof Dict, vars?: Record<string, string | number>): string => {
    let s = (dict[key] as string) ?? (zh[key] as string) ?? String(key)
    if (vars) {
      for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, String(v))
    }
    return s
  }
}

/** Resolve the card language: card config `lang` > extension-level
 *  `ui.language` (the GLOBAL setting from the extension config panel) > zh.
 *  The global value is fetched by each card and passed in. */
export function resolveLang(config: Record<string, unknown> | undefined, globalLang?: string): Lang {
  // English is the product default; Chinese is opt-in
  const card = (config?.lang as string) || (globalLang as string) || 'en'
  return card === 'zh' ? 'zh' : 'en'
}

/** React hook: card config wins over the extension-level global. */
export function useLang(
  config?: Record<string, unknown>,
  globalLang?: string
) {
  const lang = resolveLang(config, globalLang)
  const t = useMemo(() => translator(lang), [lang])
  return { lang, t }
}
