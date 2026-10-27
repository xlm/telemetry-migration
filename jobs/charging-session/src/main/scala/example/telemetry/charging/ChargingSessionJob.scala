package example.telemetry.charging

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths, StandardCopyOption}

import scala.collection.JavaConverters._
import scala.collection.mutable.ArrayBuffer

import org.apache.spark.sql.{Dataset, SparkSession}
import org.apache.spark.sql.functions.{coalesce, col, lit}
import org.apache.spark.sql.streaming.{GroupStateTimeout, OutputMode, Trigger}

import example.telemetry.charging.model._
import example.telemetry.charging.state.VehicleState

object ChargingSessionJob {
  def main(args: Array[String]): Unit = {
    require(args.length == 2, "expected fixture and golden directories")
    val fixtureDir = Paths.get(args(0))
    val goldenDir = Paths.get(args(1))
    Files.createDirectories(goldenDir)

    val spark = SparkSession
      .builder()
      .appName("charging-session")
      .config("spark.sql.shuffle.partitions", "2")
      .config("spark.ui.enabled", "false")
      .config("spark.sql.session.timeZone", "UTC")
      .config("spark.sql.jsonGenerator.ignoreNullFields", "false")
      .getOrCreate()

    try {
      import spark.implicits._

      fixtureFiles(fixtureDir).foreach { fixture =>
        val summaries = ArrayBuffer.empty[SessionSummary]
        val dlqs = ArrayBuffer.empty[DlqRecord]
        val input = Files.createTempDirectory("charging-session-input-")
        val checkpoint = Files.createTempDirectory("charging-session-checkpoint-")

        try {
          Files.copy(
            fixture,
            input.resolve(fixture.getFileName),
            StandardCopyOption.REPLACE_EXISTING
          )

          val raw = spark.readStream
            .textFile(input.toString)
            .filter(_.trim.nonEmpty)
            .map(RawChargingEvent.fromJson)
          val validated = raw.map(Validation.validate)
          val invalid = validated
            .filter(_.event.isEmpty)
            .map(result => Emission(None, result.dlq))
          val valid = validated
            .filter(_.event.isDefined)
            .map(_.event.get)

          val aggregated = valid
            .groupByKey(_.vehicle_id)
            .flatMapGroupsWithState[VehicleState, Emission](
              OutputMode.Update(),
              GroupStateTimeout.NoTimeout()
            )(SessionStateFunction.apply _)

          val emissions = aggregated.union(invalid)
          val query = emissions.writeStream
            .outputMode("update")
            .trigger(Trigger.AvailableNow())
            .option("checkpointLocation", checkpoint.toString)
            .foreachBatch { (batch: Dataset[Emission], _: Long) =>
              batch.collect().foreach { emission =>
                emission.summary.foreach(summaries += _)
                emission.dlq.foreach(dlqs += _)
              }
            }
            .start()

          try query.awaitTermination()
          finally query.stop()

          val sortedSummaries = spark
            .createDataset(summaries.toSeq)
            .orderBy(col("session_id"))
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

          writeLines(goldenDir.resolve(s"${stem(fixture)}.sessions.jsonl"), sortedSummaries)
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
