package example.telemetry.charging.state

import example.telemetry.charging.util.DedupKey

case class VehicleState(
    clock: Long,
    openSessions: Map[String, SessionState],
    closedSessionIds: Seq[String],
    seenDedupKeys: Seq[DedupKey]
) {
  def advanceClock(eventTs: Long): VehicleState =
    copy(clock = math.max(clock, eventTs))
}

object VehicleState {
  val empty: VehicleState = VehicleState(Long.MinValue, Map.empty, Seq.empty, Seq.empty)
}
