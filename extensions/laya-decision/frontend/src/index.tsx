/**
 * LAYA Decision Layer — dashboard status card.
 *
 * Live view via the platform's authoritative endpoint
 * (GET /api/decisions/status): mode badge (shadow = record-only /
 * guidance = LLM hints), backend id, and the decision/cache/error
 * counters. Polls on refreshInterval; degrades to the "off" state when
 * the layer is absent (non-decision build or no artifacts).
 */

import { forwardRef, useEffect, useState, useCallback } from 'react'

// ============================================================================
// Host-provided types (see EXTENSION_FRONTEND_DESIGN_GUIDE.md)
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
  refreshInterval?: number
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}

interface DecisionStatus {
  enabled?: boolean
  reason?: string
  available?: boolean
  shadow?: boolean
  backend?: string
  decides_total?: number
  cache_hits_total?: number
  errors_total?: number
}

// ============================================================================
// API — the platform's session-authenticated surface
// ============================================================================

const getApiHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const fetchStatus = async (): Promise<DecisionStatus> => {
  const base =
    (window as any).__NEOMIND_API_BASE__ ||
    (import.meta as any).env?.VITE_API_BASE ||
    ''
  const res = await fetch(`${base}/api/decisions/status`, { headers: getApiHeaders() })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  const body = await res.json()
  return body?.data ?? body
}

// ============================================================================
// Card
// ============================================================================

const label = {
  en: {
    title: 'Decision Layer (LAYA)',
    builtin: 'built-in',
    shadow: 'shadow · record only',
    guidance: 'guidance · LLM hints',
    off: 'not enabled',
    offHint: 'Place model artifacts under <data>/models/decision/ on a decision-native build',
    unavailable: 'model unavailable',
    decides: 'decisions',
    cache: 'cache hits',
    errors: 'errors',
  },
  zh: {
    title: '决策层 (LAYA)',
    builtin: '内置模块',
    shadow: '影子模式 · 只记录',
    guidance: '引导模式 · 注入 LLM',
    off: '未启用',
    offHint: '放置模型产物到 <数据目录>/models/decision/ 并以 decision-native 构建后自动启用',
    unavailable: '模型不可用',
    decides: '决策',
    cache: '缓存命中',
    errors: '错误',
  },
}

const pickLang = (): 'en' | 'zh' =>
  (typeof navigator !== 'undefined' && navigator.language || 'en').toLowerCase().startsWith('zh')
    ? 'zh'
    : 'en'

export const LayaDecisionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  ({ title, className, config }, ref) => {
    const [status, setStatus] = useState<DecisionStatus | null>(null)
    const [failed, setFailed] = useState(false)
    const [tick, setTick] = useState(0)

    const refresh = useCallback(() => setTick((t) => t + 1), [])

    useEffect(() => {
      let cancelled = false
      fetchStatus()
        .then((s) => !cancelled && (setStatus(s), setFailed(false)))
        .catch(() => !cancelled && setFailed(true))
      return () => {
        cancelled = true
      }
    }, [tick])

    const interval = Number(config?.refreshInterval ?? 10000)
    useEffect(() => {
      const t = setInterval(() => setTick((v) => v + 1), Math.max(2000, interval))
      return () => clearInterval(t)
    }, [interval])

    const L = label[pickLang()]
    const enabled = status?.enabled === true
    const unavailable = enabled && status?.available === false

    return (
      <div
        ref={ref}
        className={`laya-card ${className ?? ''}`}
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 8,
          padding: 16,
          height: '100%',
          boxSizing: 'border-box',
          fontFamily: 'inherit',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 15, fontWeight: 600 }}>{title ?? L.title}</span>
          <span
            style={{
              fontSize: 11,
              padding: '1px 6px',
              borderRadius: 4,
              background: 'rgba(127,127,127,0.15)',
              color: 'inherit',
              opacity: 0.8,
            }}
          >
            {L.builtin}
          </span>
          <div style={{ flex: 1 }} />
          {status?.backend && (
            <span style={{ fontSize: 11, opacity: 0.7, fontFamily: 'monospace' }}>
              {status.backend}
            </span>
          )}
        </div>

        {!enabled ? (
          <div style={{ fontSize: 12, opacity: 0.7 }}>
            {failed ? L.off : status?.reason ? `${L.off} — ${status.reason}` : L.offHint}
          </div>
        ) : (
          <>
            <div>
              <span
                style={{
                  fontSize: 12,
                  padding: '2px 8px',
                  borderRadius: 999,
                  background: unavailable
                    ? 'rgba(217,119,6,0.15)'
                    : status?.shadow
                      ? 'rgba(127,127,127,0.15)'
                      : 'rgba(59,130,246,0.15)',
                }}
              >
                {unavailable ? L.unavailable : status?.shadow ? L.shadow : L.guidance}
              </span>
            </div>
            <div style={{ display: 'flex', gap: 16, marginTop: 'auto' }}>
              {[
                [L.decides, status?.decides_total ?? 0],
                [L.cache, status?.cache_hits_total ?? 0],
                [L.errors, status?.errors_total ?? 0],
              ].map(([k, v]) => (
                <div key={String(k)} style={{ display: 'flex', flexDirection: 'column' }}>
                  <span style={{ fontSize: 18, fontWeight: 600 }}>{String(v)}</span>
                  <span style={{ fontSize: 11, opacity: 0.65 }}>{String(k)}</span>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    )
  },
)
LayaDecisionCard.displayName = 'LayaDecisionCard'
