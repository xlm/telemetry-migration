package example.telemetry.charging.model

case class SessionSummary(
    session_id: String,
    vehicle_id: String,
    start_ts: Long,
    end_ts: Long,
    duration_s: Int,
    start_soc_pct: Option[Double],
    end_soc_pct: Option[Double],
    energy_kwh: Double,
    peak_power_kw: Double,
    event_count: Int,
    close_reason: String,
    schema_version: String
)
