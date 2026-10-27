package example.telemetry.charging

import scala.math.BigDecimal.RoundingMode

import example.telemetry.charging.model.{ChargingEvent, SessionSummary}
import example.telemetry.charging.state.SessionState

object Summaries {
  def build(
      sessionId: String,
      vehicleId: String,
      session: SessionState,
      closeReason: String
  ): SessionSummary = {
    val events = session.events.sortBy(event => (event.event_ts, event.event_id))
    val startTs = events.map(_.event_ts).min
    val endTs = events.map(_.event_ts).max
    val socValues = events.filter(_.soc_pct.isDefined)
    val energy = integrate(events)
    val peakPower = events.flatMap(_.power_kw).foldLeft(0.0)(math.max)

    SessionSummary(
      sessionId,
      vehicleId,
      startTs,
      endTs,
      ((endTs - startTs) / 1000L).toInt,
      socValues.headOption.flatMap(_.soc_pct),
      socValues.lastOption.flatMap(_.soc_pct),
      BigDecimal(energy).setScale(3, RoundingMode.HALF_EVEN).toDouble,
      peakPower,
      events.size,
      closeReason,
      "1"
    )
  }

  private def integrate(events: Seq[ChargingEvent]): Double =
    events.sliding(2).foldLeft(0.0) {
      case (total, Seq(first, second))
          if first.charging_state == "CHARGING" =>
        val firstPower = first.power_kw.getOrElse(0.0)
        val secondPower = second.power_kw.getOrElse(0.0)
        total + 0.5 * (firstPower + secondPower) *
          (second.event_ts - first.event_ts).toDouble / 3600000.0
      case (total, _) => total
    }
}
