package example.telemetry.charging.model

import scala.collection.JavaConverters._
import scala.util.control.NonFatal

import com.fasterxml.jackson.databind.JsonNode
import com.fasterxml.jackson.databind.ObjectMapper

/** Input fields as they appear in the fixture records. */
case class RawChargingEvent(
    event_id: Option[String],
    vehicle_id: Option[String],
    session_id: Option[String],
    event_ts: Option[Long],
    ingest_ts: Option[Long],
    seq: Option[Long],
    charging_state: Option[String],
    soc_pct: Option[Double],
    power_kw: Option[Double],
    location: Option[Location],
    schema_version: Option[String],
    _corrupt_record: Option[String] = None
)

object RawChargingEvent {
  private val inputFields = Seq(
    "event_id",
    "vehicle_id",
    "session_id",
    "event_ts",
    "ingest_ts",
    "seq",
    "charging_state",
    "soc_pct",
    "power_kw",
    "location",
    "schema_version"
  )
  private val mapper = new ObjectMapper()

  def fromJson(line: String): RawChargingEvent = {
    val root = try
      Option(mapper.readTree(line))
        .filter { node =>
          node.isObject &&
          !inputFields.exists(name => Option(node.get(name)).exists(hasNonFiniteNumber))
        }
    catch {
      case NonFatal(_) => None
    }

    root match {
      case None =>
        RawChargingEvent(
          None,
          None,
          None,
          None,
          None,
          None,
          None,
          None,
          None,
          None,
          None,
          Some(line)
        )
      case Some(node) =>
        RawChargingEvent(
          textField(node, "event_id"),
          textField(node, "vehicle_id"),
          textField(node, "session_id"),
          longField(node, "event_ts"),
          longField(node, "ingest_ts"),
          longField(node, "seq"),
          textField(node, "charging_state"),
          numberField(node, "soc_pct"),
          numberField(node, "power_kw"),
          locationField(node.get("location")),
          textField(node, "schema_version")
        )
    }
  }

  private def textField(node: JsonNode, name: String): Option[String] =
    Option(node.get(name)).filter(_.isTextual).map(_.textValue())

  private def longField(node: JsonNode, name: String): Option[Long] =
    Option(node.get(name))
      .filter(field => field.isIntegralNumber && field.canConvertToLong)
      .map(_.longValue())

  private def numberField(node: JsonNode, name: String): Option[Double] =
    Option(node.get(name)).filter(_.isNumber).map(_.doubleValue())

  private def hasNonFiniteNumber(node: JsonNode): Boolean =
    if (node.isFloatingPointNumber) !java.lang.Double.isFinite(node.doubleValue())
    else if (node.isContainerNode) node.elements().asScala.exists(hasNonFiniteNumber)
    else false

  private def locationField(node: JsonNode): Option[Location] =
    Option(node)
      .filter(_.isObject)
      .flatMap { location =>
        for {
          lat <- numberField(location, "lat")
          lon <- numberField(location, "lon")
        } yield Location(Some(lat), Some(lon))
      }
}
