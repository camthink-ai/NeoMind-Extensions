// DeepStream extension — small badge that pairs an icon with a numeric value.
//
// Used by OverviewCard / StreamCard to surface per-stream and aggregate counts
// (active streams, total detections, persons, vehicles, etc.). The chip is a
// rounded pill that flexes around its content; pass any ReactNode as the icon
// (typically one of the inline SVGs from ./icons).
//
// Styling follows EXTENSION_FRONTEND_DESIGN_GUIDE.md §2.3 (badge) — pure CSS,
// no Tailwind, all colors via CSS variables so light/dark mode just works.

import type { ReactNode } from 'react';

export interface CountChipProps {
  /** Leading icon (usually an inline SVG from ./icons). Omit for value-only chip. */
  icon?: ReactNode;
  /** Numeric count or short string. */
  value: number | string;
  /** Tooltip / aria-label text. */
  label?: string;
  className?: string;
}

const STYLES = `
.ds-count-chip {
  /* Map host CSS variables to chip-local aliases — see DESIGN_GUIDE §5. */
  --ds-chip-fg: var(--foreground);
  --ds-chip-muted: var(--muted-foreground);
  --ds-chip-bg: var(--secondary);
  --ds-chip-border: var(--border);

  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: var(--radius-full, 9999px);
  background: var(--ds-chip-bg);
  color: var(--ds-chip-fg);
  border: 1px solid var(--ds-chip-border);
  font-size: 11px;
  font-weight: 500;
  line-height: 1.4;
  white-space: nowrap;
  user-select: none;
}

.ds-count-chip__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  color: var(--ds-chip-muted);
  flex-shrink: 0;
}

.ds-count-chip__icon svg {
  width: 14px;
  height: 14px;
}

.ds-count-chip__value {
  font-variant-numeric: tabular-nums;
}
`;

export function CountChip({ icon, value, label, className }: CountChipProps) {
  return (
    <>
      <style>{STYLES}</style>
      <div
        className={`ds-count-chip ${className ?? ''}`}
        title={label}
        role={label ? 'img' : undefined}
        aria-label={label}
      >
        {icon != null ? <span className="ds-count-chip__icon">{icon}</span> : null}
        <span className="ds-count-chip__value">{value}</span>
      </div>
    </>
  );
}

export default CountChip;
