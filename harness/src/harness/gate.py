"""Run all six Gate steps in order and collect findings without stopping early.

The steps are Harness, Contract, Downstream System, Privacy, protected-path
check, and Evidence Pack generation. The path check compares against
origin/main by default, or --base when supplied, and protects goldens/,
fixtures/, manifests/*.job.yaml, harness/, contracts/, jobs/, and downstream/.
Continue after failures so all steps report results. Exit nonzero if any step
is red; otherwise exit 0.
"""
