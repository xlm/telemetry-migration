package example.telemetry.charging

import org.scalatest.funsuite.AnyFunSuite

import example.telemetry.charging.model.{ChargingEvent, DlqRecord, RawChargingEvent}
import example.telemetry.charging.state.VehicleState

class ChargingSessionSpec extends AnyFunSuite {
  test("energy uses trapezoids and half-even rounding") {
    val events = Seq(
      event("a", "CS-1", "CHARGING", 0L, 1L, 1L, Some(20.0), Some(74.07)),
      event("b", "CS-1", "COMPLETE", 60000L, 2L, 2L, Some(21.0), Some(74.07))
    )

    val (_, output) = SessionStateFunction.process(VehicleState.empty, events)
    val summary = output.flatMap(_.summary).head

    assert(summary.energy_kwh == 1.234)
    assert(summary.duration_s == 60)
    assert(summary.start_soc_pct.contains(20.0))
    assert(summary.end_soc_pct.contains(21.0))
  }

  test("validation returns the first failing reason") {
    val missing = Validation.validate(
      RawChargingEvent(None, Some("VH-1"), Some("CS-1"), Some(1L), Some(1L), Some(1L), Some("INVALID"), Some(-1.0), Some(-1.0), None, Some("1"))
    )
    val unknown = Validation.validate(
      RawChargingEvent(Some("e"), Some("VH-1"), None, Some(1L), Some(1L), Some(1L), Some("INVALID"), Some(-1.0), Some(-1.0), None, Some("1"))
    )
    val soc = Validation.validate(
      RawChargingEvent(Some("e"), Some("VH-1"), Some("CS-1"), Some(1L), Some(1L), Some(1L), Some("CHARGING"), Some(101.0), Some(-1.0), None, Some("1"))
    )
    val power = Validation.validate(
      RawChargingEvent(Some("e"), Some("VH-1"), Some("CS-1"), Some(1L), Some(1L), Some(1L), Some("CHARGING"), Some(50.0), Some(-1.0), None, Some("1"))
    )

    assert(missing.dlq.map(_.reason).contains("MISSING_FIELD"))
    assert(unknown.dlq.map(_.reason).contains("UNKNOWN_STATE"))
    assert(soc.dlq.map(_.reason).contains("SOC_OUT_OF_RANGE"))
    assert(power.dlq.map(_.reason).contains("NEGATIVE_POWER"))

    val corrupt = Validation.validate(
      RawChargingEvent(
        Some("partial"),
        Some("VH-1"),
        Some("CS-1"),
        Some(1L),
        Some(1L),
        Some(1L),
        Some("CHARGING"),
        Some(50.0),
        Some(10.0),
        None,
        Some("1"),
        Some("{")
      )
    )
    assert(corrupt.dlq.contains(DlqRecord(null, null, "MISSING_FIELD")))
  }

  test("wrong-typed optional fields are absent and invalid JSON is marked corrupt") {
    val raw = RawChargingEvent.fromJson(
      """{"event_id":"e","vehicle_id":"VH-1","session_id":"CS-1","event_ts":1,"ingest_ts":1,"seq":1,"charging_state":"CHARGING","soc_pct":"25","power_kw":10.0,"location":{"lat":"1","lon":2}}"""
    )
    val invalidJson = RawChargingEvent.fromJson("""{"event_id":""")
    val nonObject = RawChargingEvent.fromJson("[]")
    val outOfRangeNumber = RawChargingEvent.fromJson(
      """{"event_id":"e","vehicle_id":"VH-1","session_id":"CS-1","event_ts":1,"ingest_ts":1,"seq":1,"charging_state":"CHARGING","soc_pct":1e400,"power_kw":10.0}"""
    )
    val overflowInUnknownField = RawChargingEvent.fromJson(
      """{"event_id":"e","vehicle_id":"VH-1","session_id":"CS-1","event_ts":1,"ingest_ts":1,"seq":1,"charging_state":"CHARGING","soc_pct":25,"power_kw":10.0,"extra":1e400}"""
    )
    val overflowInLocation = RawChargingEvent.fromJson(
      """{"event_id":"e","vehicle_id":"VH-1","session_id":"CS-1","event_ts":1,"ingest_ts":1,"seq":1,"charging_state":"CHARGING","soc_pct":25,"power_kw":10.0,"location":{"lat":1e400,"lon":2}}"""
    )

    assert(raw.soc_pct.isEmpty)
    assert(raw.location.isEmpty)
    assert(Validation.validate(raw).event.isDefined)
    assert(invalidJson._corrupt_record.contains("""{"event_id":"""))
    assert(nonObject._corrupt_record.contains("[]"))
    assert(outOfRangeNumber._corrupt_record.isDefined)
    assert(overflowInUnknownField._corrupt_record.isEmpty)
    assert(Validation.validate(overflowInUnknownField).event.isDefined)
    assert(overflowInLocation._corrupt_record.isDefined)
  }

  test("duplicate sequence is rejected and later event time closes idle session") {
    val events = Seq(
      event("a", "CS-A", "PLUGGED_IN", 1000L, 1L, 1L, Some(0.0), None),
      event("duplicate", "CS-A", "CHARGING", 1001L, 2L, 1L, Some(20.0), Some(10.0)),
      event("b", "CS-B", "PLUGGED_IN", 1801000L, 3L, 2L, Some(0.0), None)
    )

    val (_, output) = SessionStateFunction.process(VehicleState.empty, events)

    assert(output.flatMap(_.dlq).map(_.reason) == Seq("DUPLICATE"))
    assert(output.flatMap(_.summary).map(_.session_id) == Seq("CS-A"))
    assert(output.flatMap(_.summary).head.close_reason == "TIMEOUT")
  }

  private def event(
      id: String,
      session: String,
      state: String,
      eventTs: Long,
      ingestTs: Long,
      seq: Long,
      soc: Option[Double],
      power: Option[Double]
  ): ChargingEvent =
    ChargingEvent(id, "VH-1", Some(session), eventTs, ingestTs, seq, state, soc, power)
}
