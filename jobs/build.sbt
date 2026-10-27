ThisBuild / scalaVersion := "2.12.20"
ThisBuild / version := "0.1.0"
ThisBuild / organization := "example.telemetry"

val sparkVersion = "3.5.5"
val jdk17Options = Seq(
  "--add-opens=java.base/java.lang=ALL-UNNAMED",
  "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED",
  "--add-opens=java.base/java.nio=ALL-UNNAMED",
  "--add-opens=java.base/java.util=ALL-UNNAMED",
  "--add-opens=java.base/java.util.concurrent=ALL-UNNAMED",
  "--add-opens=java.base/java.util.concurrent.atomic=ALL-UNNAMED",
  "--add-opens=java.base/sun.nio.ch=ALL-UNNAMED"
)

lazy val commonSettings = Seq(
  libraryDependencies += "org.apache.spark" %% "spark-sql" % sparkVersion,
  libraryDependencies += "org.scalatest" %% "scalatest" % "3.2.19" % Test,
  Compile / run / fork := true,
  Compile / run / javaOptions ++= jdk17Options,
  Compile / run / javaOptions += "-Dspark.master=local[2]",
  Test / fork := true,
  Test / javaOptions ++= jdk17Options,
  Test / javaOptions += "-Dspark.master=local[2]"
)

lazy val otaStatus = project
  .in(file("ota-status"))
  .settings(commonSettings)

lazy val chargingSession = project
  .in(file("charging-session"))
  .settings(commonSettings)

lazy val root = project
  .in(file("."))
  .aggregate(otaStatus, chargingSession)
  .settings(
    publish / skip := true
  )
