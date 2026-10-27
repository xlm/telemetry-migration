package example.telemetry.charging.model

/** A charging event with the fields required by the state transition. */
case class ChargingEvent(
    event_id: String,
    vehicle_id: String,
    session_id: Option[String],
    event_ts: Long,
    ingest_ts: Long,
    seq: Long,
    charging_state: String,
    soc_pct: Option[Double],
    power_kw: Option[Double]
)
