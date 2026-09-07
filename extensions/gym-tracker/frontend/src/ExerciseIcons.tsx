/**
 * Exercise icons — one line-art glyph per exercise family, drawn with the
 * same 24×24 stroke language as the card headers (stroke 2, round caps).
 * `exIcon(name)` maps an exercise label to its glyph; unmapped labels get
 * the generic activity icon.
 */
import type { JSX } from 'react'

const S = { fill: 'none', stroke: 'currentColor', strokeWidth: 2,
            strokeLinecap: 'round', strokeLinejoin: 'round' } as const

/** generic activity (fallback) */
const Activity = () => (
  <svg viewBox="0 0 24 24" {...S}><polyline points="22 12 18 12 15 21 9 3 6 12 2 12" /></svg>
)
/** running (treadmill / run) */
const Run = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="15" cy="4.5" r="1.8" />
    <path d="M12.5 8.5 9 11l2 3-1.5 5" />
    <path d="M12.5 8.5 16 10l3 .5M11 14l4 1 2.5 4M9 11 5 12l-2 3" />
  </svg>
)
/** walking */
const Walk = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="14" cy="4.5" r="1.8" />
    <path d="M11.5 21 13 16l-2.5-3 .5-4 3 2 3 .5M13 16l-1 5M11 9 8.5 11l-1.5 3" />
  </svg>
)
/** squat / lunge / leg press family (knee bend) */
const Squat = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="12" cy="4" r="1.8" />
    <path d="M12 6v5l-4 3M12 11l4 3" />
    <path d="M7 19c1.5-2.5 3-3.5 5-3.5s3.5 1 5 3.5" />
  </svg>
)
/** bench press / pushup (lying press) */
const Bench = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M4 10h16M6 10v-3M18 10V7M6 7h12" />
    <circle cx="10" cy="15" r="2.2" />
    <path d="M4 19h16" />
  </svg>
)
/** dumbbell / curl / raise (weight in hand) */
const Dumbbell = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M6.5 6.5v11M17.5 6.5v11M3.5 9v6M20.5 9v6M6.5 12h11" />
  </svg>
)
/** cycling / spin bike */
const Bike = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="6" cy="17" r="3.5" /><circle cx="18" cy="17" r="3.5" />
    <path d="M6 17l4-7h5l3 7M10 10l2-4h3M8 6h4" />
  </svg>
)
/** rowing machine */
const Row = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="18" cy="5" r="1.8" />
    <path d="M17 8l-6 5-5-2M11 13l-1 6M4 21l4-2" />
    <path d="M14 12l2 9" />
  </svg>
)
/** pull-up (bar + raised body) */
const Pullup = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M4 4h16M12 4v4" />
    <path d="M12 8c-1.5 2-2.5 4-2.5 6.5S10.5 19 12 20c1.5-1 2.5-3 2.5-5.5S13.5 10 12 8z" />
  </svg>
)
/** crunch / situp (mat + torso curl) */
const Crunch = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M3 19h18" />
    <circle cx="9" cy="8" r="1.8" />
    <path d="M9 10c-2 1-4 3-5 6M13 12l5 1" />
  </svg>
)
/** plank (static hold) */
const Plank = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M3 15h18" />
    <circle cx="7" cy="11" r="1.8" />
    <path d="M9 12l9 3M5 15l-1 3M19 15l1 3" />
  </svg>
)
/** stairs (stair climber) */
const Stairs = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M4 20h4v-4h4v-4h4V8h4" />
  </svg>
)
/** stretch / yoga */
const Stretch = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <circle cx="12" cy="4" r="1.8" />
    <path d="M12 6v7M12 13l-4 3M12 13l4 3M8 16l-2 4M16 16l2 4" />
  </svg>
)
/** deadlift (hip hinge) */
const Deadlift = () => (
  <svg viewBox="0 0 24 24" {...S}>
    <path d="M5 9v10M19 9v10M5 9h14M5 19h14" />
    <circle cx="12" cy="5" r="1.8" /><path d="M10 8l-1 5 3 3 3-3-1-5" />
  </svg>
)

export type IconCmp = () => JSX.Element

const EX_ICON: Record<string, IconCmp> = {
  treadmill_run: Run, run: Run, walk: Walk,
  squat: Squat, lunge: Squat, leg_press: Squat, kettlebell_swing: Squat,
  bench_press: Bench, pushup: Bench, chest_fly: Bench,
  bicep_curl: Dumbbell, lateral_raise: Dumbbell, shoulder_press: Dumbbell,
  dumbbell_row: Dumbbell, lat_pulldown: Dumbbell,
  spin_bike: Bike, cycling: Bike, elliptical: Bike,
  rowing: Row, pullup: Pullup,
  crunch: Crunch, situp: Crunch, plank: Plank,
  stair_climber: Stairs,
  stretch: Stretch, deadlift: Deadlift,
  standing: Walk, unknown: Activity, pending: Activity,
}

export function exIcon(name: string): IconCmp {
  return EX_ICON[name] ?? Activity
}
