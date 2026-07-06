// DeepStream extension — inline SVG icon set (spec §5.4.1).
//
// All icons are stroke-based with strokeWidth=1.5 on a 20x20 viewBox, except
// `StatusDotIcon` and `ChipIcon` which are filled/background-style.
//
// Each icon is a functional component that spreads its props onto the root
// <svg> element so callers can pass className, style, aria-label, etc.
//
// NO emoji and NO icon-library imports — pure inline SVG paths referencing
// `currentColor` so icons inherit text color from CSS.

import type { SVGProps } from 'react';

const BASE_PROPS = {
  width: 20,
  height: 20,
  viewBox: '0 0 20 20',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.5,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

// 1. Video camera (lens + body).
export function CameraIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <rect x="2" y="5" width="16" height="11" rx="2" />
      <circle cx="10" cy="10.5" r="3" />
      <path d="M7.5 5 L8.5 3 L11.5 3 L12.5 5" />
    </svg>
  );
}

// 2. Speedometer / gauge (semicircle dial + needle).
export function GaugeIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M3 13 A7 7 0 0 1 17 13" />
      <path d="M10 13 L13 9" />
      <circle cx="10" cy="13" r="1" />
      <path d="M3 13 L4.2 12.2" />
      <path d="M17 13 L15.8 12.2" />
      <path d="M10 6 L10 7.4" />
    </svg>
  );
}

// 3. Person silhouette (head + shoulders).
export function PersonIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <circle cx="10" cy="6.5" r="2.8" />
      <path d="M3.5 17 A6.5 6.5 0 0 1 16.5 17" />
    </svg>
  );
}

// 4. Car silhouette (covers car/truck/bus via color/style variants —
// do NOT add a separate BusIcon).
export function CarIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M2.5 13 L2.5 10.5 L4.5 6.5 L15.5 6.5 L17.5 10.5 L17.5 13" />
      <path d="M2.5 13 L17.5 13" />
      <path d="M2.5 13 L2.5 15.5" />
      <path d="M17.5 13 L17.5 15.5" />
      <path d="M4.5 13 A1.2 1.2 0 0 1 2.1 13" />
      <path d="M17.9 13 A1.2 1.2 0 0 1 15.5 13" />
      <path d="M5 10.5 L15 10.5" />
    </svg>
  );
}

// 5. Warning triangle with exclamation.
export function AlertTriangleIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M10 3 L18 16.5 A1 1 0 0 1 17.1 18 L2.9 18 A1 1 0 0 1 2 16.5 Z" />
      <path d="M10 8 L10 12.5" />
      <path d="M10 15 L10 15.1" />
    </svg>
  );
}

// 6. Right-pointing arrow (for line-crossing direction).
export function ArrowRightIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M3 10 L16.5 10" />
      <path d="M11 4.5 L16.5 10 L11 15.5" />
    </svg>
  );
}

// 7. Clipboard / copy icon.
export function CopyIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <rect x="6" y="6" width="11" height="11" rx="2" />
      <path d="M3.5 13.5 A1.5 1.5 0 0 1 2 12 L2 4 A2 2 0 0 1 4 2 L11 2 A1.5 1.5 0 0 1 12.5 3.5" />
    </svg>
  );
}

// 8. Plus / add icon.
export function PlusIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M10 3 L10 17" />
      <path d="M3 10 L17 10" />
    </svg>
  );
}

// 9. Circular arrow (refresh / retry).
export function RefreshIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...BASE_PROPS} {...props}>
      <path d="M16 6 A7 7 0 1 0 17.5 13" />
      <path d="M16 2.5 L16 6 L12.5 6" />
    </svg>
  );
}

// 10. Solid filled circle (status indicator).
export function StatusDotIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width={20} height={20} viewBox="0 0 20 20" fill="none" {...props}>
      <circle cx="10" cy="10" r="5" fill="currentColor" stroke="none" />
    </svg>
  );
}

// 11. Chip / IC rectangle with pins (for count chip background).
export function ChipIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width={20} height={20} viewBox="0 0 20 20" fill="none" {...props}>
      <rect
        x="4.5"
        y="4.5"
        width="11"
        height="11"
        rx="1.5"
        fill="currentColor"
        stroke="none"
      />
      <path
        d="M7 4.5 L7 2.8 M10 4.5 L10 2.8 M13 4.5 L13 2.8"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
      />
      <path
        d="M7 17.2 L7 15.5 M10 17.2 L10 15.5 M13 17.2 L13 15.5"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
      />
      <path
        d="M4.5 7 L2.8 7 M4.5 10 L2.8 10 M4.5 13 L2.8 13"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
      />
      <path
        d="M17.2 7 L15.5 7 M17.2 10 L15.5 10 M17.2 13 L15.5 13"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
      />
    </svg>
  );
}
