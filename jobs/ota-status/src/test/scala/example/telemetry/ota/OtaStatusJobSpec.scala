package example.telemetry.ota

import org.scalatest.funsuite.AnyFunSuite

class OtaStatusJobSpec extends AnyFunSuite {
  test("rejects wrong-typed fields while preserving available identifiers") {
    val stringProgress = OtaStatusJob.classify(
      """{"event_id":"e1","vehicle_id":"v1","campaign_id":"c1","status":"dl","progress_pct":"25","event_ts":10}"""
    )
    val numericId = OtaStatusJob.classify(
      """{"event_id":123,"vehicle_id":"v1","campaign_id":"c1","status":"dl","progress_pct":25,"event_ts":10}"""
    )

    assert(stringProgress == Left(OtaStatusJob.DlqRecord("e1", "v1", "MISSING_FIELD")))
    assert(numericId == Left(OtaStatusJob.DlqRecord(null, "v1", "MISSING_FIELD")))
  }

  test("accepts numeric progress and maps parse failures to null identifiers") {
    val valid = OtaStatusJob.classify(
      """{"event_id":"e1","vehicle_id":"v1","campaign_id":"c1","status":"dl","progress_pct":25,"event_ts":10}"""
    )
    val overflowInUnknownField = OtaStatusJob.classify(
      """{"event_id":"e1","vehicle_id":"v1","campaign_id":"c1","status":"dl","progress_pct":25,"event_ts":10,"extra":1e400}"""
    )
    val invalidJson = OtaStatusJob.classify("""{"event_id":""")
    val nonObject = OtaStatusJob.classify("[]")
    val outOfRangeNumber = OtaStatusJob.classify(
      """{"event_id":"e1","vehicle_id":"v1","campaign_id":"c1","status":"dl","progress_pct":1e400,"event_ts":10}"""
    )

    assert(
      valid == Right(
        OtaStatusJob.StatusRecord("e1", "v1", "c1", "DOWNLOADING", 25.0, 10L, "1")
      )
    )
    assert(overflowInUnknownField == valid)
    assert(invalidJson == Left(OtaStatusJob.DlqRecord(null, null, "MISSING_FIELD")))
    assert(nonObject == Left(OtaStatusJob.DlqRecord(null, null, "MISSING_FIELD")))
    assert(outOfRangeNumber == Left(OtaStatusJob.DlqRecord(null, null, "MISSING_FIELD")))
  }
}
