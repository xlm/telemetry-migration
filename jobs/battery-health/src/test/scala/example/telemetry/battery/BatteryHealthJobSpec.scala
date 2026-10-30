package example.telemetry.battery

import org.scalatest.funsuite.AnyFunSuite

class BatteryHealthJobSpec extends AnyFunSuite {
  private def event(soh: String): String =
    s"""{"event_id":"e1","vehicle_id":"VH-12345678","battery_id":"b1","soh_pct":$soh,"event_ts":10}"""

  test("clamps health and preserves exact threshold boundaries") {
    for ((input, expected, health) <- Seq(
      ("-1", 0.0, "REPLACE"), ("0", 0.0, "REPLACE"),
      ("59.999", 59.999, "REPLACE"), ("60", 60.0, "DEGRADED"),
      ("79.999", 79.999, "DEGRADED"), ("80", 80.0, "HEALTHY"),
      ("100", 100.0, "HEALTHY"), ("101", 100.0, "HEALTHY")
    )) {
      assert(BatteryHealthJob.classify(event(input)) == Right(
        BatteryHealthJob.HealthRecord("e1", "VH-12345678", "b1", expected, health, 10L, "1")
      ))
    }
  }

  test("rejects missing, null and wrong-typed required fields with available identifiers") {
    val mapper = new com.fasterxml.jackson.databind.ObjectMapper()
    for (field <- Seq("event_id", "vehicle_id", "battery_id", "soh_pct", "event_ts")) {
      for (value <- Seq("missing", "null", "wrong-type")) {
        val node = mapper.readTree(event("90")).deepCopy[com.fasterxml.jackson.databind.node.ObjectNode]()
        value match {
          case "missing" => node.remove(field)
          case "null" => node.putNull(field)
          case _ if field == "soh_pct" || field == "event_ts" => node.put(field, "90")
          case _ => node.put(field, 90)
        }
        assert(BatteryHealthJob.classify(node.toString) == Left(
          BatteryHealthJob.DlqRecord(
            if (field == "event_id") null else "e1",
            if (field == "vehicle_id") null else "VH-12345678", "MISSING_FIELD"
          )
        ))
      }
    }
  }

  test("rejects invalid JSON, non-objects, non-finite declared values and invalid timestamps") {
    for (line <- Seq("{", "[]", "null", event("1e400"))) {
      assert(BatteryHealthJob.classify(line) == Left(
        BatteryHealthJob.DlqRecord(null, null, "MISSING_FIELD")
      ))
    }
    for (timestamp <- Seq("10.5", "9223372036854775808")) {
      val input = event("90").replace("\"event_ts\":10", "\"event_ts\":" + timestamp)
      assert(BatteryHealthJob.classify(input) == Left(
        BatteryHealthJob.DlqRecord("e1", "VH-12345678", "MISSING_FIELD")
      ))
    }
  }

  test("ignores unknown fields and emits schema version one without rounding") {
    val input = event("80.12345")
    val extended = input.dropRight(1) + """, "schema_version":"99", "location":{"lat":12}, "extra":1e400}"""
    assert(BatteryHealthJob.classify(input) == BatteryHealthJob.classify(extended))
    assert(BatteryHealthJob.classify(input).right.get.soh_pct == 80.12345)
    assert(BatteryHealthJob.classify(input).right.get.schema_version == "1")
  }
}
