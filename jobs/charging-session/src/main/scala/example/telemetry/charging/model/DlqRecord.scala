package example.telemetry.charging.model

case class DlqRecord(
    event_id: String,
    vehicle_id: String,
    reason: String
)
