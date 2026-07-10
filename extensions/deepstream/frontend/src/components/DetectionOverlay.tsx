// DetectionOverlay — HTML div bounding-box overlay positioned over a video.
//
// Detection events carry bbox = [x1, y1, x2, y2] in pixel space (1920x1080).
// We convert to percentage positions so the overlay tracks the video at any
// rendered size.

import { memo, useMemo } from 'react';

export interface DetectedObject {
  class: number;
  conf: number;
  track_id?: number;
  bbox: [number, number, number, number];
}

export interface DetectionOverlayProps {
  objects: DetectedObject[];
  refWidth?: number;
  refHeight?: number;
  showLabels?: boolean;
}

// NVIDIA TrafficCamnet labels (config_infer_Primary_Detector):
//   0=Car  1=Bicycle  2=Person  3=Roadsign
const CLASS_NAMES: Record<number, string> = {
  0: 'Car',
  1: 'Bicycle',
  2: 'Person',
  3: 'Roadsign',
};

const CLASS_COLORS: Record<number, string> = {
  0: '#22c55e',   // Car — green
  2: '#3b82f6',   // Person — blue
  3: '#f59e0b',   // Roadsign — amber
};
const DEFAULT_COLOR = '#a855f7';

export const DetectionOverlay = memo(function DetectionOverlay({
  objects,
  refWidth = 1920,
  refHeight = 1080,
  showLabels = true,
}: DetectionOverlayProps) {
  const boxes = useMemo(() => {
    return objects
      .filter((o) => o.conf >= 0.3)
      .map((o) => {
        const [x1, y1, x2, y2] = o.bbox;
        return {
          key: `${o.track_id ?? ''}-${o.class}-${x1.toFixed(0)}-${y1.toFixed(0)}`,
          left: (x1 / refWidth) * 100,
          top: (y1 / refHeight) * 100,
          width: Math.max((x2 - x1) / refWidth * 100, 0.5),
          height: Math.max((y2 - y1) / refHeight * 100, 0.5),
          label: CLASS_NAMES[o.class] ?? `#${o.class}`,
          conf: o.conf,
          color: CLASS_COLORS[o.class] ?? DEFAULT_COLOR,
        };
      });
  }, [objects, refWidth, refHeight]);

  if (boxes.length === 0) return null;

  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        pointerEvents: 'none',
        zIndex: 5,
      }}
    >
      {boxes.map((b) => (
        <div
          key={b.key}
          style={{
            position: 'absolute',
            left: `${b.left}%`,
            top: `${b.top}%`,
            width: `${b.width}%`,
            height: `${b.height}%`,
            border: `2px solid ${b.color}`,
            boxSizing: 'border-box',
            borderRadius: 2,
          }}
        >
          {showLabels && (
            <span
              style={{
                position: 'absolute',
                top: -16,
                left: 0,
                fontSize: 10,
                lineHeight: '14px',
                padding: '1px 4px',
                borderRadius: 2,
                background: b.color,
                color: '#fff',
                fontFamily: 'ui-monospace, monospace',
                whiteSpace: 'nowrap',
              }}
            >
              {b.label} {(b.conf * 100).toFixed(0)}%
            </span>
          )}
        </div>
      ))}
    </div>
  );
});

export default DetectionOverlay;
