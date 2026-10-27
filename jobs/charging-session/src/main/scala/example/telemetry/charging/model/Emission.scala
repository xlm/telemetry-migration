package example.telemetry.charging.model

case class ValidationResult(
    event: Option[ChargingEvent],
    dlq: Option[DlqRecord]
)

case class Emission(
    summary: Option[SessionSummary],
    dlq: Option[DlqRecord]
)
