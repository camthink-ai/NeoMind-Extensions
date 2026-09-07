/**
 * Gym Tracker — frontend entry (UMD).
 *
 * One extension bundle, four business components:
 *  - GymLiveState     presence + track list + ROI zone editor
 *  - GymEquipmentGrid equipment-zone occupancy board
 *  - GymTrafficChart  presence trend over the last N hours
 *  - GymVideoOverlay  live video + bbox/skeleton overlay
 *
 * The host loads `gym-tracker-components.umd.cjs` and reads the named
 * exports (or the default object map). React / ReactDOM are external —
 * provided by the host app, NOT bundled.
 */

import { GymLiveState } from './GymLiveState'
import { GymEquipmentGrid } from './GymEquipmentGrid'
import { GymTrafficChart } from './GymTrafficChart'
import { GymVideoOverlay } from './GymVideoOverlay'
import { GymWorkoutSummary } from './GymWorkoutSummary'
import { GymEquipmentRank } from './GymEquipmentRank'
import { GymMemberReport } from './GymMemberReport'
import { GymAlerts } from './GymAlerts'
import { GymDoorFlow } from './GymDoorFlow'
import { GymTrailsCard, GymHeatCard } from './GymActivityMap'

export { GymLiveState, GymEquipmentGrid, GymTrafficChart, GymVideoOverlay, GymWorkoutSummary, GymEquipmentRank, GymMemberReport, GymAlerts, GymDoorFlow, GymTrailsCard, GymHeatCard }
export type { ExtensionComponentProps, DataSource } from './common'

export default { GymLiveState, GymEquipmentGrid, GymTrafficChart, GymVideoOverlay, GymWorkoutSummary, GymEquipmentRank, GymMemberReport, GymAlerts, GymDoorFlow, GymTrailsCard, GymHeatCard }
