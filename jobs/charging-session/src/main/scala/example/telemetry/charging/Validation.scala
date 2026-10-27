package example.telemetry.charging

import example.telemetry.charging.model._

object Validation {
  private val supportedStates = Set(
    "PLUGGED_IN",
    "CHARGING",
    "PAUSED",
    "COMPLETE",
    "UNPLUGGED",
    "HEARTBEAT"
  )

  def validate(raw: RawChargingEvent): ValidationResult = {
    val missingRequired =
      raw.event_id.isEmpty || raw.vehicle_id.isEmpty || raw.event_ts.isEmpty ||
        raw.charging_state.isEmpty || raw.ingest_ts.isEmpty || raw.seq.isEmpty

    if (raw._corrupt_record.isDefined) {
      ValidationResult(None, Some(DlqRecord(null, null, "MISSING_FIELD")))
    } else if (missingRequired) {
      invalid(raw, "MISSING_FIELD")
    } else if (!supportedStates.contains(raw.charging_state.get)) {
      invalid(raw, "UNKNOWN_STATE")
    } else if (raw.session_id.isEmpty && raw.charging_state.get != "HEARTBEAT") {
      invalid(raw, "MISSING_FIELD")
    } else if (raw.soc_pct.exists(value => value < 0.0 || value > 100.0)) {
      invalid(raw, "SOC_OUT_OF_RANGE")
    } else if (raw.power_kw.exists(_ < 0.0)) {
      invalid(raw, "NEGATIVE_POWER")
    } else {
      ValidationResult(
        Some(
          ChargingEvent(
            raw.event_id.get,
            raw.vehicle_id.get,
            raw.session_id,
            raw.event_ts.get,
            raw.ingest_ts.get,
            raw.seq.get,
            raw.charging_state.get,
            raw.soc_pct,
            raw.power_kw
          )
        ),
        None
      )
    }
  }

  private def invalid(raw: RawChargingEvent, reason: String): ValidationResult =
    ValidationResult(
      None,
      Some(DlqRecord(raw.event_id.orNull, raw.vehicle_id.orNull, reason))
    )
}
