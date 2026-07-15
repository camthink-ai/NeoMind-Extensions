/**
 * Gym Tracker — frontend entry (UMD).
 *
 * Host loads `gym-tracker-components.umd.cjs` and reads the named export
 * `GymLiveState` (or the default object map). React / ReactDOM are external —
 * provided by the host app, NOT bundled.
 */

import { GymLiveState } from './GymLiveState'

export { GymLiveState }
export type { ExtensionComponentProps, DataSource } from './GymLiveState'

export default { GymLiveState }
