/**
 * LAYA Decision Layer — dashboard status card.
 *
 * Reads the extension's OWN `status` command via the platform's generic
 * extension-command endpoint (POST /extensions/laya-decision/command) —
 * self-sufficient by design since the platform's decision endpoints were
 * removed (the layer is 100% extension-owned now). Shows: engine state,
 * artifact size, sidecar port, and the audit counter. Polls on
 * refreshInterval; degrades to the "artifacts missing" state with the
 * install hint when model files are absent.
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
  model_present?: boolean
  model_size_bytes?: number
  decisions_logged?: number
  last_decision?: any
  install_hint?: string | null
  sidecar?: {
    port?: number
    url?: string
    engine_up?: boolean
  }
}

// ============================================================================
// API — the platform's session-authenticated extension-command surface
// ============================================================================

const getApiHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const fetchStatus = async (extensionId: string): Promise<DecisionStatus> => {
  const base =
    (window as any).__NEOMIND_API_BASE__ ||
    (import.meta as any).env?.VITE_API_BASE ||
    ''
  const res = await fetch(`${base}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: getApiHeaders(),
    body: JSON.stringify({ command: 'status', args: {} }),
  })
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
    engine: 'engine · onnx',
    ready: 'ready',
    artifactsMissing: 'model artifacts missing',
    extensionError: 'extension unreachable',
    offHint:
      'This extension hosts the decision runtime — decide/status commands work right after install; place the model artifacts under <data>/models/decision/ to go live',
    decides: 'decisions logged',
    size: 'model size',
  },
  zh: {
    title: '决策层 (LAYA)',
    engine: '引擎 · ONNX',
    ready: '就绪',
    artifactsMissing: '模型文件缺失',
    extensionError: '扩展不可达',
    offHint:
      '本扩展即决策运行时——安装后 decide/status 命令即可用；将模型产物放入 <data>/models/decision/ 后引擎自动启动',
    decides: '已记录决策',
    size: '模型大小',
  },
}

const pickLang = (): 'en' | 'zh' =>
  (typeof navigator !== 'undefined' && navigator.language || 'en').toLowerCase().startsWith('zh')
    ? 'zh'
    : 'en'

const fmtSize = (b?: number): string => {
  if (!b) return '—'
  if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(2)} GB`
  if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(0)} MB`
  return `${(b / (1 << 10)).toFixed(0)} KB`
}

export const LayaDecisionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  ({ title, dataSource, className, config }, ref) => {
    const [status, setStatus] = useState<DecisionStatus | null>(null)
    const [failed, setFailed] = useState(false)
    const [tick, setTick] = useState(0)

    const refresh = useCallback(() => setTick((t) => t + 1), [])

    useEffect(() => {
      let cancelled = false
      const extensionId = dataSource?.extensionId || 'laya-decision'
      fetchStatus(extensionId)
        .then((s) => !cancelled && (setStatus(s), setFailed(false)))
        .catch(() => !cancelled && setFailed(true))
      return () => {
        cancelled = true
      }
    }, [tick, dataSource?.extensionId])

    const interval = Number(config?.refreshInterval ?? 10000)
    useEffect(() => {
      const t = setInterval(() => setTick((v) => v + 1), Math.max(2000, interval))
      return () => clearInterval(t)
    }, [interval])

    const L = label[pickLang()]
    const present = status?.model_present === true
    const engineUp = present && status?.sidecar?.engine_up === true

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
          <div style={{ flex: 1 }} />
          {status?.sidecar?.port ? (
            <span style={{ fontSize: 11, opacity: 0.7, fontFamily: 'monospace' }}>
              127.0.0.1:{status.sidecar.port}
            </span>
          ) : null}
        </div>

        {failed ? (
          <div style={{ fontSize: 12, opacity: 0.7 }}>{L.extensionError}</div>
        ) : !present ? (
          <div style={{ fontSize: 12, opacity: 0.7 }}>{L.offHint}</div>
        ) : (
          <>
            <div>
              <span
                style={{
                  fontSize: 12,
                  padding: '2px 8px',
                  borderRadius: 999,
                  background: engineUp
                    ? 'rgba(59,130,246,0.15)'
                    : 'rgba(217,119,6,0.15)',
                }}
              >
                {L.engine} · {engineUp ? L.ready : L.artifactsMissing}
              </span>
            </div>
            <div style={{ display: 'flex', gap: 16, marginTop: 'auto' }}>
              {[
                [L.decides, status?.decisions_logged ?? 0],
                [L.size, fmtSize(status?.model_size_bytes)],
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
