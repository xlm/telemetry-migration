package example.telemetry.ota

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths, StandardCopyOption}
import java.util.Locale

import scala.collection.JavaConverters._
import scala.collection.mutable.ArrayBuffer
import scala.util.control.NonFatal

import com.fasterxml.jackson.databind.{JsonNode, ObjectMapper}
import org.apache.spark.sql.{Dataset, SparkSession}
import org.apache.spark.sql.functions.{coalesce, col, lit}
import org.apache.spark.sql.streaming.Trigger

object OtaStatusJob {
  private val inputFields = Seq(
    "event_id",
    "vehicle_id",
    "campaign_id",
    "status",
    "progress_pct",
    "event_ts",
    "schema_version"
  )
  private val mapper = new ObjectMapper()

  case class StatusRecord(
      event_id: String,
      vehicle_id: String,
      campaign_id: String,
      status: String,
      progress_pct: Double,
      event_ts: Long,
      schema_version: String
  )

  case class DlqRecord(
      event_id: String,
      vehicle_id: String,
      reason: String
  )

  def main(args: Array[String]): Unit = {
    require(args.length == 2, "expected fixture and golden directories")
    val fixtureDir = Paths.get(args(0))
    val goldenDir = Paths.get(args(1))
    Files.createDirectories(goldenDir)

    val spark = SparkSession
      .builder()
      .appName("ota-status")
      .config("spark.sql.shuffle.partitions", "2")
      .config("spark.ui.enabled", "false")
      .config("spark.sql.session.timeZone", "UTC")
      .config("spark.sql.jsonGenerator.ignoreNullFields", "false")
      .getOrCreate()

    try {
      import spark.implicits._

      fixtureFiles(fixtureDir).foreach { fixture =>
        val statuses = ArrayBuffer.empty[StatusRecord]
        val dlqs = ArrayBuffer.empty[DlqRecord]
        val input = Files.createTempDirectory("ota-status-input-")
        val checkpoint = Files.createTempDirectory("ota-status-checkpoint-")

        try {
          Files.copy(
            fixture,
            input.resolve(fixture.getFileName),
            StandardCopyOption.REPLACE_EXISTING
          )

          val lines = spark.readStream.textFile(input.toString).filter(_.trim.nonEmpty)
          val query = lines.writeStream
            .trigger(Trigger.AvailableNow())
            .option("checkpointLocation", checkpoint.toString)
            .foreachBatch { (batch: Dataset[String], _: Long) =>
              batch.collect().foreach { line =>
                classify(line) match {
                  case Left(dlq)     => dlqs += dlq
                  case Right(status) => statuses += status
                }
              }
            }
            .start()

          try query.awaitTermination()
          finally query.stop()

          val sortedStatuses = spark
            .createDataset(statuses.toSeq)
            .orderBy(col("event_ts"), col("event_id"))
            .coalesce(1)
            .toJSON
            .collect()
            .toSeq
          val sortedDlqs = spark
            .createDataset(dlqs.toSeq)
            .orderBy(
              coalesce(col("event_id"), lit("")),
              coalesce(col("vehicle_id"), lit("")),
              col("reason")
            )
            .coalesce(1)
            .toJSON
            .collect()
            .toSeq

          writeLines(goldenDir.resolve(s"${stem(fixture)}.status.jsonl"), sortedStatuses)
          writeLines(goldenDir.resolve(s"${stem(fixture)}.dlq.jsonl"), sortedDlqs)
        } finally {
          deleteTree(input)
          deleteTree(checkpoint)
        }
      }
    } finally {
      spark.stop()
    }
  }

  private[ota] def classify(line: String): Either[DlqRecord, StatusRecord] = {
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
      case None => Left(DlqRecord(null, null, "MISSING_FIELD"))
      case Some(node) =>
        val eventId = textField(node, "event_id")
        val vehicleId = textField(node, "vehicle_id")
        val campaignId = textField(node, "campaign_id")
        val rawStatus = textField(node, "status")
        val progress = numberField(node, "progress_pct")
        val eventTs = longField(node, "event_ts")

        if (
          eventId.isEmpty || vehicleId.isEmpty || campaignId.isEmpty || rawStatus.isEmpty ||
          eventTs.isEmpty || progress.isEmpty
        ) {
          Left(DlqRecord(eventId.orNull, vehicleId.orNull, "MISSING_FIELD"))
        } else {
          normalizeStatus(rawStatus.get) match {
            case None =>
              Left(DlqRecord(eventId.orNull, vehicleId.orNull, "UNKNOWN_STATUS"))
            case Some(status) =>
              Right(
                StatusRecord(
                  eventId.get,
                  vehicleId.get,
                  campaignId.get,
                  status,
                  math.max(0.0, math.min(100.0, progress.get)),
                  eventTs.get,
                  "1"
                )
              )
          }
        }
    }
  }

  private def textField(node: JsonNode, name: String): Option[String] =
    Option(node.get(name)).filter(_.isTextual).map(_.textValue())

  private def numberField(node: JsonNode, name: String): Option[Double] =
    Option(node.get(name)).filter(_.isNumber).map(_.doubleValue())

  private def longField(node: JsonNode, name: String): Option[Long] =
    Option(node.get(name))
      .filter(field => field.isIntegralNumber && field.canConvertToLong)
      .map(_.longValue())

  private def hasNonFiniteNumber(node: JsonNode): Boolean =
    if (node.isFloatingPointNumber) !java.lang.Double.isFinite(node.doubleValue())
    else if (node.isContainerNode) node.elements().asScala.exists(hasNonFiniteNumber)
    else false

  private def normalizeStatus(value: String): Option[String] =
    value.toLowerCase(Locale.ROOT) match {
      case "downloading" | "dl" | "download" => Some("DOWNLOADING")
      case "installing" | "inst"             => Some("INSTALLING")
      case "done" | "complete" | "success" | "succeeded" =>
        Some("SUCCEEDED")
      case "failed" | "error" => Some("FAILED")
      case _                  => None
    }

  private def fixtureFiles(dir: Path): Seq[Path] = {
    val files = Files.list(dir)
    try {
      files.iterator().asScala
        .filter(path => Files.isRegularFile(path) && path.getFileName.toString.endsWith(".jsonl"))
        .toSeq
        .sortBy(_.getFileName.toString)
    } finally files.close()
  }

  private def stem(path: Path): String = {
    val name = path.getFileName.toString
    name.substring(0, name.length - ".jsonl".length)
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
