# Run of show - "The second Job"

Audience: VP of Engineering (Software Platforms), Chief Architect (Connected Vehicle & Cloud).
Slot: 10-15 minutes plus questions. Format: hybrid (live session kicked off at minute 0, pre-run session narrated while it works).
Vocabulary: see `CONTEXT.md`. Use those words in the room, especially Job / Processor / Downstream System / Evidence Pack / Gate / Drift.

## One-sentence thesis

The expensive part of moving the remaining Jobs is not writing Rust, it is proving to engineering and privacy reviewers that each Processor is safe. Devin does the Migration inside a Gate that produces that proof, and the same skill does it for every Job after the first.

## Screens you need open before walking in

1. Devin cloud, new-session page, repo `telemetry-migration`, branch `drift/dedup-and-idle` selected.
2. GitHub: the open PR from the pre-run migration session (`demo/charging-session-migrated` into `main`), scrolled to the Evidence Pack in the description.
3. Devin cloud: the completed pre-run migration session, scrolled to the top.
4. Devin cloud: the completed backup run of the live prompt (in case the live one is slow).
5. GitHub: `.agents/skills/migrate-job/SKILL.md` rendered.
6. GitHub: `AGENTS.md` rendered, scrolled to "Gate inputs are read-only".
7. Terminal in a local clone, on `violation/location-in-summary`, with `make gate PROCESSOR=charging-session` typed but not run.
8. `docs/cutover.md` rendered.
9. DeepWiki page for the repo (optional, objection 5).

Do not have a slide deck open. The repo and the sessions are the deck.

## Minute by minute

### 0:00 - 1:00 Frame and kick off the live session

Say: "You have one Job on Rust and the rest still on Spark. The blocker is per-Job proof, not per-Job code. I am going to start Devin on a real task now and come back to it."

Paste the live prompt (below) into screen 1. Start the session. Do not watch it.

### 1:00 - 3:00 The repo in 90 seconds

Screen 2 (GitHub tree). Point at four things only:

- `jobs/` - the Scala/Spark Jobs. `ota-status` (already migrated), `charging-session` (the one we are doing).
- `processors/` - the Rust workspace. `ota-status` is the finished exemplar: pure core, file adapter, Kafka adapter behind a feature flag.
- `manifests/` - one YAML per Job and per Processor: what fields are read, derived, emitted, logged, and their classification.
- `evidence/ota-status/` - the Evidence Pack the first Migration produced. "This is what done looks like. The question is how to get the next one for the price of a prompt."

If the Architect asks how Devin learns an undocumented Job: screen 9, DeepWiki, ten seconds.

### 3:00 - 7:00 The pre-run Migration

Screen 5 for 30 seconds: the `migrate-job` skill. Read the step headings, the stop conditions and the acceptance criteria out loud. "Every Job from here on starts with this. It is a file in the repo, so your architects review changes to the process the same way they review code."

Screen 3: the completed session. Scroll, do not read. Narrate the shape:

1. Devin read the Scala and wrote down the business rules it found (point at the characterisation notes it produced).
2. It implemented the Rust Processor against the existing skeleton.
3. It ran the Gate. Show the first red Gate if there was one in the session, then the green one.
4. It generated the Evidence Pack and opened the PR.

Screen 2: the PR. Three things: diff is confined to `processors/charging-session/` and `manifests/charging-session.processor.yaml`. Evidence Pack in the description. Devin Review findings below it. "Devin Review is a second agent reviewing the first. A human still merges."

Say the honest sentence once: "The Gate proves equivalence on these fixtures, under these checks. It does not prove production safety. What it does is turn a two-week discovery exercise into a 20-minute read."

### 7:00 - 9:00 The Gate going red for privacy

Screen 7. Say: "Here is a branch where the port looks fine and the Harness is green, but the Processor copies the vehicle's last location onto the session summary." Run `make gate PROCESSOR=charging-session`. Red. Point at the finding: field `end_location`, classification `sensitive-location`, not permitted on topic `vehicle.charging.sessions`, Baseline unchanged, disposition `policy violation - requires remediation`.

Say: "Nobody rewrote the policy to make this pass, and the Evidence Pack tells a privacy reviewer exactly which field and which rule. That is what 'no lengthy review per Job' actually means: the reviewer reads this, not the code."

### 9:00 - 11:00 Back to the live session

Screen 1. Three outcomes:

- **Finished**: show the PR diff (only Rust files touched), read Devin's root-cause explanation of both Drifts, run `make gate` on its branch locally. Green. Best ending.
- **Mid-flight**: show where it is (usually it has found the dedup key by now; the Vehicle Clock takes longer because the port's wall-clock version looks reasonable until you read the Scala). Say "I ran this exact prompt yesterday" and switch to screen 4 for the ending. Come back to screen 1 at the end of Q&A if it has finished.
- **Stuck or wrong**: say so plainly, switch to screen 4. Do not rescue it live.

Whichever happens, screen 6 for ten seconds: the read-only section of `AGENTS.md`. "This is injected into every session on this repo. It is why the diff only touches Rust, and the Gate checks the same list mechanically."

### 11:00 - 13:00 Repeat for Job #3 and what humans keep

Screen 5 again, one sentence: "Battery-health is next. `@skills:migrate-job battery-health`. Same procedure, different argument."

Then what humans keep, in this order: architectural approval, privacy sign-off on the Evidence Pack, the cutover decision. Screen 8, `docs/cutover.md`, one sentence: "Shadow consumer group, shadow topic, idempotent producer, offset handoff. Documented, not demonstrated, because it depends on your deployment pipeline, which already exists."

### 13:00 - 15:00 Questions

Objection script below.

## Prompts, verbatim

### Live prompt (screen 1, branch `drift/dedup-and-idle`)

```
The migration gate for the charging-session processor is red on this branch.

Run `make gate PROCESSOR=charging-session`, find the root cause of every
failure, and fix it in the Rust processor under processors/charging-session/.

Constraints:
- Do not modify anything under harness/, fixtures/, goldens/, manifests/,
  contracts/, jobs/ or downstream/. If you believe one of those is wrong,
  stop and say so.
- The Scala job in jobs/charging-session/ is the source of truth for
  behaviour. Read it before changing the Rust.

When the gate is green, open a PR that explains, for each drift you found:
what the legacy job actually does, what the Rust did instead, and how you
verified the fix.
```

### Pre-run prompt (screen 3, branch `main`)

```
@skills:migrate-job charging-session
```

The skill body lives at `.agents/skills/migrate-job/SKILL.md` in the demo repo.

## Objection script

Lead with the artefact, then the sentence. Never argue the general case.

| # | Objection | Artefact | Sentence |
|---|---|---|---|
| 1 | "How do we trust a Rust rewrite is behaviourally identical?" | Harness output, `goldens/` generated from the Scala Job, the Drift branch | "The Goldens come from running your Job, not from a spec. The Harness compares per session key, with DLQ and a Downstream System check. The Drift branch shows two schema-valid, wrong ports being caught." |
| 2 | "Devin will just edit the tests until they pass." | `AGENTS.md` (screen 6), live PR diff, Gate path check | "Three layers. `AGENTS.md` is injected into every session and names the read-only paths. The skill repeats it. The Gate diffs the branch against those paths mechanically and goes red if any were touched. If it had edited Goldens, the PR and the Gate would both show it." |
| 3 | "You didn't show it on Kafka. What about exactly-once?" | `processors/*/src/core.rs`, `docs/cutover.md` | "Ordering, dedup and timeout are properties of the core, and the core is tested deterministically, which a live broker cannot do. Transport, offsets and rebalances are the adapter and your pipeline, exercised by Job #1 already. Exactly-once here means Kafka-to-Kafka under transactional produce, and the doc says what that does and does not cover." |
| 4 | "Our privacy team won't accept machine-generated evidence." | Evidence Pack on the PR, violation branch output | "It is not a decision, it is a dossier. Field-level inventory, lineage, Baseline diff, runtime scan, with the residual uncertainties listed. Your reviewer still signs. They just read 20 minutes instead of discovering for two weeks." |
| 5 | "Our real Jobs are far messier than this." | DeepWiki (screen 9), skill stop conditions | "Agreed. That is why the skill's first step is characterisation, not code, and why it has explicit stop conditions. Some Jobs will stop at step one with a findings doc, and that is still a week saved." |
| 6 | "Where does our code run? What does Devin see?" | none | Answer from Cognition's security material: deployment options, data retention, what is and is not used for training. Do not improvise. |
| 7 | "What is the review burden on our senior engineers?" | PR size, Devin Review, Evidence Pack structure | "One PR per Job, confined to one Processor directory, with a second agent's review attached and a structured Evidence Pack. Review becomes reading, not archaeology." |
| 8 | "How many Jobs per week? What does it cost?" | none | Be candid: it depends on Job complexity and how many stop at characterisation. Offer a pilot on two real Jobs as the way to find out. |
| 9 | (unspoken) The stack is not exactly Rust-on-Kubernetes | none | "Nothing here is Rust-specific. The Gate, Manifests and the skill are the method. The target language is a parameter." |

## What this demo proves and does not prove

Say these if asked, and never claim more.

Proves:
- A Processor matches its Job on the committed fixtures, per session key, including DLQ and a Downstream System observable.
- Devin can port a stateful Job inside a Gate without touching the Gate's inputs.
- Semantic Drift (schema-valid, wrong values) is caught, not just schema breaks.
- A privacy-relevant change is detected from Manifest diff plus runtime scan, and reported in a form a reviewer can act on.
- The same skill runs for the next Job.

Does not prove:
- Production safety under load, rebalance, restart or broker failure.
- Exactly-once end to end into non-Kafka sinks.
- That the fixtures cover every behaviour of the real Job.
- That source-level lineage equals runtime data flow (the runtime scan narrows this, it does not close it).
- That any privacy or legal review obligation is removed.

## Failure modes in the room and what to do

| Failure | Action |
|---|---|
| Wi-Fi dies | Terminal only. `make gate` on `main` for ota-status (green), `drift/dedup-and-idle` (red), `violation/location-in-summary` (red), `demo/charging-session-migrated` (green). Narrate the sessions from memory. |
| Live session slow | Screen 4. Say it is yesterday's run of the same prompt. |
| Live session edits a read-only path | It should not, because of `AGENTS.md`, but if it does: show it, say "this is exactly what Devin Review and the Gate's path check are for", switch to screen 4. |
| Someone asks to see the Scala run | `make goldens JOB=charging-session` in the terminal. Takes about a minute in Spark local mode. Only if you smoke-tested it that morning. |
| Someone asks to see Kafka | Only if the Redpanda compose was smoke-tested that morning. Otherwise objection 3. |

## Pre-meeting checklist (morning of)

- [ ] `make doctor` prints `doctor: ok` on this laptop.
- [ ] `git fetch`, every demo branch at its recorded SHA (see `docs/build-plan.md`, "Demo states").
- [ ] `make gate PROCESSOR=ota-status` green on `main`.
- [ ] `make gate PROCESSOR=charging-session` red on `drift/dedup-and-idle` on the Harness for fixtures 04 and 09 and on the Downstream System for fixture 09 only, every other step green.
- [ ] `make gate PROCESSOR=charging-session` red on `violation/location-in-summary` on contract and privacy, both naming `end_location`.
- [ ] Pre-run PR still open, Devin Review findings visible.
- [ ] Backup live-prompt session (screen 4) opens and scrolls.
- [ ] No stray `devin/*` branches or PRs from rehearsals.
- [ ] `.agents/skills/migrate-job/SKILL.md` and `AGENTS.md` read end to end.
- [ ] Laptop on power, notifications off, terminal font large.
