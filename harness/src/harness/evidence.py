"""Build the JSON and Markdown Evidence Pack for a Migration.

Include identity (processor, commit SHA, manifest versions, timestamp, and
reproduction command), inventory, Baseline change assessment, validation
counts for Gate steps 1-5, disposition, and residual uncertainties. The
inventory comes from the manifest and identifies exact artifacts. The worst
finding determines disposition. List fixture coverage, value-based rather
than flow-based runtime scanning, and untested broker semantics as residual
uncertainties. Keep the Markdown report under 150 lines and never claim
artifacts or checks that were not produced.
"""
