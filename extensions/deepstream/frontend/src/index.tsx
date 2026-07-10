// DeepStream extension frontend entry point.
// Exports the single comprehensive management card consumed by the NeoMind
// dashboard, plus the AddStreamForm (reused internally + by consumers).
//
// Bundle output: dist/deepstream-components.umd.cjs (UMD)
// Global: window.DeepStreamComponents

export { DeepStreamManagerCard } from './components/ManagerCard';
export { AddStreamForm } from './components/AddStreamForm';

// Re-export icons + types so downstream consumers (and smoke tests) can reach
// them via the UMD global.
export * from './components/icons';
export type * from './types';

export const __version = '2.7.7';
