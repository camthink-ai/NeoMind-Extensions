// DeepStream extension frontend entry point.
// Exports the 4 React components consumed by the NeoMind dashboard.
//
// Bundle output: dist/deepstream-components.umd.cjs (UMD)
// Global: window.DeepStreamComponents

export { DeepStreamStatsCard } from './components/StatsCard';
export { DeepStreamOverviewCard } from './components/OverviewCard';
export { DeepStreamStreamCard } from './components/StreamCard';
export { AddStreamForm } from './components/AddStreamForm';

// Re-export icons + types so downstream consumers (and Storybook-style smoke
// tests) can reach them via the UMD global.
export * from './components/icons';
export type * from './types';

export const __version = '2.7.7';
