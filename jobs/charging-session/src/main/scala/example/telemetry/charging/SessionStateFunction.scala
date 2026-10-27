package example.telemetry.charging

import scala.collection.mutable.ArrayBuffer

import org.apache.spark.sql.streaming.GroupState

import example.telemetry.charging.config.JobConfig
import example.telemetry.charging.model.{ChargingEvent, DlqRecord, Emission}
import example.telemetry.charging.state.{SessionState, VehicleState}
import example.telemetry.charging.util.DedupKey

object SessionStateFunction {
  def apply(
      vehicleId: String,
      events: Iterator[ChargingEvent],
      groupState: GroupState[VehicleState]
  ): Iterator[Emission] = {
    val initial = if (groupState.exists) groupState.get else VehicleState.empty
    val (next, output) = process(initial, events.toSeq)
    groupState.update(next)
    output.iterator
  }

  def process(
      initial: VehicleState,
      events: Seq[ChargingEvent]
  ): (VehicleState, Seq[Emission]) = {
    var state = initial
    val output = ArrayBuffer.empty[Emission]

    events.sortBy(event => (event.ingest_ts, event.event_id)).foreach { event =>
      state = state.advanceClock(event.event_ts)
      val expired = state.openSessions.toSeq
        .filter { case (_, session) =>
          session.lastEventTs + JobConfig.SessionIdleTimeout <= state.clock
        }
        .map(_._1)
        .sorted

      expired.foreach { sessionId =>
        val session = state.openSessions(sessionId)
        output += Emission(
          Some(Summaries.build(sessionId, event.vehicle_id, session, "TIMEOUT")),
          None
        )
        state = state.copy(
          openSessions = state.openSessions - sessionId,
          closedSessionIds = state.closedSessionIds :+ sessionId
        )
      }

      if (event.charging_state != "HEARTBEAT") {
        val dedupKey = DedupKey(event.vehicle_id, event.seq)
        if (state.seenDedupKeys.contains(dedupKey)) {
          output += Emission(
            None,
            Some(DlqRecord(event.event_id, event.vehicle_id, "DUPLICATE"))
          )
        } else if (event.session_id.exists(state.closedSessionIds.contains)) {
          output += Emission(
            None,
            Some(DlqRecord(event.event_id, event.vehicle_id, "LATE"))
          )
        } else {
          val sessionId = event.session_id.get
          val session = state.openSessions.getOrElse(
            sessionId,
            SessionState(Seq.empty, event.event_ts)
          )
          val updatedSession = session.copy(
            events = session.events :+ event,
            lastEventTs = math.max(session.lastEventTs, event.event_ts)
          )
          state = state.copy(
            openSessions = state.openSessions.updated(sessionId, updatedSession),
            seenDedupKeys = state.seenDedupKeys :+ dedupKey
          )

          if (event.charging_state == "COMPLETE" || event.charging_state == "UNPLUGGED") {
            output += Emission(
              Some(Summaries.build(sessionId, event.vehicle_id, updatedSession, event.charging_state)),
              None
            )
            state = state.copy(
              openSessions = state.openSessions - sessionId,
              closedSessionIds = state.closedSessionIds :+ sessionId
            )
          }
        }
      }
    }

    (state, output.toSeq)
  }
}
