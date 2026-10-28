# One Evidence Pack, headline first, for reviewers and executives

The Evidence Pack has two audiences: the privacy or engineering reviewer who decides whether a Migration stays within the Baseline, and engineering leadership, who skim it on a PR page. We kept one pack instead of adding an executive summary file, because the pack is pasted verbatim into the PR description, shown on the `2. Gate: <name>` job summary and committed, and a second artefact would drift from it. The first screen answers one question, "can this Migration go to human review, and if not, what blocks it?", using an alert headline, a five-cell status strip, plain-English Baseline changes and failures, and the residual uncertainties. Everything the reviewer relied on before is kept, unchanged, inside a closed "Reviewer detail" `<details>`.

## Decisions

- The headline says "ready / not ready for human review", never approved or safe, because the Gate is not approval.
- When several steps are red, the headline leads with the first in this order: Path check, Privacy, Harness, Contract, Downstream System. Path check comes first because a changed Gate input means no other result can be trusted.
- Plain-English sentences are generated per finding type, counted across fixtures, with one concrete example each. An unknown type falls back to its technical message, so nothing is dropped silently.
- `evidence.json` gains an additive `summary` object, and `evidence.md` is a pure render of the JSON. `3. Gate verdict` shows the headline sentence as explanation only. Green or red still comes from the `Gate:` log line.
- `evidence.md` is capped at 20,000 characters, leaving room in the 65,536-character PR body for the migrate-job skill's Drift notes and limitations.
- The identity line links the commit on GitHub and names the branch. The PR number is added when the Gate runs in GitHub Actions for a PR. A pack generated locally does not know it, and the commit page on GitHub links back to the merged PR.
- PRs stay on rebase-merge, so a committed pack names a PR commit that GitHub keeps under `refs/pull/`. The Reproduce line names no SHA, and rerun on `main` it builds the same files.

## Considered options

- Mermaid diagram or shields.io badges for the steps: rejected. Mermaid is noisy in raw text and diffs, and badges are external images. An emoji table renders everywhere and reads in raw text.
- A red pack with its technical findings open: rejected. The plain-English "What failed" section carries the red reasons, and the technical list stays one click away.

## Consequences

Changing the pack changes `harness/`, which is a Gate input. The change therefore ships as a platform PR, red at Path check by design and merged with the admin bypass, and must land before the demo branches are cut, or every demo branch needs restacking and new Evidence Pack SHAs. The PR's committed ota-status sample is generated with `BASE=HEAD`, which the identity line shows, so it is green. Against `main` it would be red at Path check because of this PR's own Gate change, not the Migration's. The old sample did effectively the same, since it compared `main` with `main`.
