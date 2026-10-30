package example.telemetry.battery

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths, StandardCopyOption}

import scala.collection.JavaConverters._
import scala.collection.mutable.ArrayBuffer
import scala.util.control.NonFatal

import com.fasterxml.jackson.databind.{JsonNode, ObjectMapper}
import org.apache.spark.sql.{Dataset, SparkSession}
import org.apache.spark.sql.functions.{coalesce, col, lit}
import org.apache.spark.sql.streaming.Trigger

object BatteryHealthJob {
  private val inputFields = Seq(
    "event_id", "vehicle_id", "battery_id", "soh_pct", "event_ts", "schema_version"
  )
  private val mapper = new ObjectMapper()

  case class HealthRecord(
      event_id: String,
      vehicle_id: String,
      battery_id: String,
      soh_pct: Double,
      health: String,
      event_ts: Long,
      schema_version: String
  )

  case class DlqRecord(event_id: String, vehicle_id: String, reason: String)

  private[battery] def classify(line: String): Either[DlqRecord, HealthRecord] = {
    val root = try
      Option(mapper.readTree(line)).filter { node =>
        node.isObject &&
        !inputFields.exists(name => Option(node.get(name)).exists(hasNonFiniteNumber))
      }
    catch {
      case NonFatal(_) => None
    }

    root match {
      case None => Left(DlqRecord(null, null, "MISSING_FIELD"))
      case Some(node) =>
        val eventId = textField(node, "event_id")
        val vehicleId = textField(node, "vehicle_id")
        val batteryId = textField(node, "battery_id")
        val soh = Option(node.get("soh_pct")).filter(_.isNumber).map(_.doubleValue())
        val timestamp = Option(node.get("event_ts"))
          .filter(field => field.isIntegralNumber && field.canConvertToLong)
          .map(_.longValue())
        if (
          eventId.isEmpty || vehicleId.isEmpty || batteryId.isEmpty ||
          soh.isEmpty || timestamp.isEmpty
        ) Left(DlqRecord(eventId.orNull, vehicleId.orNull, "MISSING_FIELD"))
        else {
          val clamped = math.max(0.0, math.min(100.0, soh.get))
          val health = if (clamped >= 80.0) "HEALTHY"
          else if (clamped >= 60.0) "DEGRADED"
          else "REPLACE"
          Right(HealthRecord(
            eventId.get, vehicleId.get, batteryId.get, clamped, health, timestamp.get, "1"
          ))
        }
    }
  }

  private def textField(node: JsonNode, name: String): Option[String] =
    Option(node.get(name)).filter(_.isTextual).map(_.textValue())

  private def hasNonFiniteNumber(node: JsonNode): Boolean =
    if (node.isFloatingPointNumber) !java.lang.Double.isFinite(node.doubleValue())
    else if (node.isContainerNode) node.elements().asScala.exists(hasNonFiniteNumber)
    else false

  def main(args: Array[String]): Unit = {
    require(args.length == 2, "expected fixture and golden directories")
    val fixtureDir = Paths.get(args(0))
    val goldenDir = Paths.get(args(1))
    Files.createDirectories(goldenDir)
    val spark = SparkSession.builder()
      .appName("battery-health")
      .config("spark.sql.shuffle.partitions", "2")
      .config("spark.ui.enabled", "false")
      .config("spark.sql.session.timeZone", "UTC")
      .config("spark.sql.jsonGenerator.ignoreNullFields", "false")
      .getOrCreate()
    try {
      import spark.implicits._
      val files = Files.list(fixtureDir)
      val fixtures = try files.iterator().asScala
        .filter(path => Files.isRegularFile(path) && path.toString.endsWith(".jsonl"))
        .toVector.sortBy(_.getFileName.toString)
      finally files.close()
      fixtures.foreach { fixture =>
        val records = ArrayBuffer.empty[HealthRecord]
        val dlqs = ArrayBuffer.empty[DlqRecord]
        val input = Files.createTempDirectory("battery-health-input-")
        val checkpoint = Files.createTempDirectory("battery-health-checkpoint-")
        try {
          Files.copy(fixture, input.resolve(fixture.getFileName), StandardCopyOption.REPLACE_EXISTING)
          val query = spark.readStream.textFile(input.toString).filter(_.trim.nonEmpty)
            .writeStream.trigger(Trigger.AvailableNow())
            .option("checkpointLocation", checkpoint.toString)
            .foreachBatch { (batch: Dataset[String], _: Long) =>
              batch.collect().foreach { line =>
                classify(line) match {
                  case Left(dlq) => dlqs += dlq
                  case Right(record) => records += record
                }
              }
            }.start()
          try query.awaitTermination()
          finally query.stop()
          val stem = fixture.getFileName.toString.stripSuffix(".jsonl")
          writeLines(goldenDir.resolve(s"$stem.health.jsonl"),
            spark.createDataset(records.toSeq).orderBy(col("event_ts"), col("event_id"))
              .coalesce(1).toJSON.collect().toSeq)
          writeLines(goldenDir.resolve(s"$stem.dlq.jsonl"),
            spark.createDataset(dlqs.toSeq).orderBy(
              coalesce(col("event_id"), lit("")), coalesce(col("vehicle_id"), lit("")), col("reason")
            ).coalesce(1).toJSON.collect().toSeq)
        } finally {
          deleteTree(input)
          deleteTree(checkpoint)
        }
      }
    } finally spark.stop()
  }

  private def writeLines(path: Path, lines: Seq[String]): Unit = {
    val content = if (lines.isEmpty) "" else lines.mkString("", "\n", "\n")
    Files.write(path, content.getBytes(StandardCharsets.UTF_8))
  }

  private def deleteTree(path: Path): Unit =
    if (Files.exists(path)) {
      val paths = Files.walk(path)
      try paths.iterator().asScala.toSeq.sortBy(_.getNameCount).reverse.foreach(Files.deleteIfExists)
      finally paths.close()
    }
}
