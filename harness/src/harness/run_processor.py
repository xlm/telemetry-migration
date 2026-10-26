"""Run processors/target/release/<name> against a fixture.

Pass --input, --output, --dlq, and --log paths and capture the process exit
code. A non-zero exit or any missing output file is a Harness finding for that
fixture. Continue Gate evaluation with whatever output files exist.
"""
