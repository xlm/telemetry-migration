package example.telemetry.charging.state

import example.telemetry.charging.model.ChargingEvent

case class SessionState(events: Seq[ChargingEvent], lastEventTs: Long)
