# Human external developer trial protocol

> Status: protocol draft (T-M9-48), ready for a maintainer to run; executing ≥ 3 real testers (T-M9-49) is an external human activity that cannot run in CI or be delegated to an agent.
> Goal: validate whether "add a custom target and produce a package within one day" is feasible for external developers, and feed issues back by severity.
> Chinese: [HUMAN-TRIAL.md](HUMAN-TRIAL.md)

## 1. Scope and outputs

- Applies to external developers who did not work on this repo and follow only [GA](GA.en.md) §2 plus [README](../README.en.md) from scratch.
- Outputs: one anonymized report per tester (template in §7), one summary issue, a fix list, and documentation write-back PRs.
- Non-goals: a formal usability lab, eye-tracking / biometric measurement, or collecting personal data unrelated to the trial.
- This work delivers the protocol and templates only; the internal rehearsal and the real trial must be run and recorded separately by maintainers.

## 2. Recruitment criteria

| Dimension | Requirement |
| --- | --- |
| Identity | Has not worked on the PolyPack repo or read internal design docs |
| Experience | At least 1 Rust background, 1 mobile / frontend background, 1 cross-platform release background |
| Environment | At least 1 clean machine each for Linux / macOS / Windows; Rust may be preinstalled |
| Time | Can commit 90 continuous minutes and agrees to think-aloud recording |
| Exclusion | Direct reporting line to a maintainer, or participation in this milestone's review |

Target sample: 3–5 people; fewer than 3 real reports means T-M9-49 cannot close.

## 3. Informed consent

Confirm and record each item before the trial starts (electronic or paper):

- Purpose and process: 90-minute task, think-aloud, screen and voice recording (only with consent).
- Voluntary participation and withdrawal: stop at any time, without explanation, with no effect on rights or payment.
- Data minimization: collect only task-related commands, timings, verbatim blockers, and environment versions; never accounts, keys, or customer data.
- Anonymization: reports use IDs (P1, P2, …), never names, companies, or email addresses; strip usernames from paths before quoting.
- Retention: raw screen / audio recordings are deleted within 90 days after the report is archived; anonymized reports may be public.
- Payment: if any, state the amount and method in writing before starting; never reduce it because of negative feedback.
- Contact and complaint channel: provide the maintainer contact and data-handling contact up front.

## 4. 90-minute task script

| Phase | Minutes | Task | Observation focus |
| --- | --- | --- | --- |
| Preparation | 10 | Check the environment, sign consent, explain think-aloud | Whether the goal and recording are understood |
| Scaffold | 20 | `polypack init` or `polypack plugin new pocket-notes`, then `polypack plugin check` | Docs entry points, first errors, wait time |
| Custom target | 35 | Implement describe / package per GA §2 and declare the custom target | Spec comprehension, plugin template, debug loop |
| Packaging | 15 | `polypack build --config <app>/polypack.toml --target pocket-notes`, then `polypack verify` | Command discoverability, artifact location, failure messages |
| Debrief | 10 | Classify per §6, finish the anonymized report, open questions | Real blockers and improvement suggestions |

Reference command flow (GA §2 and the trial project are authoritative):

```bash
polypack plugin new pocket-notes
polypack plugin check pocket-notes
polypack build --config <app>/polypack.toml --target pocket-notes
polypack verify --target pocket-notes
```

## 5. Observation and think-aloud

- Ask the tester to state intent before a command and whether the result matched expectations afterward.
- The facilitator intervenes only after 2 minutes of being stuck, for a safety / privacy issue, or when the tester asks for help.
- Record timestamps, verbatim quotes, command sequence, error codes, docs pages opened, and time spent there.
- Do not lead with solutions; ask "what are you trying to accomplish?" and "what did you expect to happen?".
- Separate "missing documentation", "command failure", and "conceptual misunderstanding" so they are not conflated.
- Write facilitator observations within 10 minutes after the trial; do not rely on later recall.

## 6. Severity classification

| Level | Definition | Example | Handling deadline |
| --- | --- | --- | --- |
| `blocker` | Cannot complete the scaffold → custom target → package core path, with no workable workaround | Template will not build, target will not load, packaging always fails | Fix or disclose before release |
| `friction` | Completable, but costs more than 10 extra minutes or the docs mislead | Hard-to-find flags, errors that do not point at the fix | Next documentation batch |
| `suggestion` | Experience improvement that does not block the task | Output wording, example order, completion hints | Enter scheduling review |

Every issue must include: steps, verbatim quote, expected / actual, whether it blocked, and evidence location (timestamp or screenshot ID).

## 7. Anonymized report template

```yaml
participant: P1
date: YYYY-MM-DD
environment:
  os: "Linux / macOS / Windows + version"
  rust: "rustc --version"
  polypack: "polypack --version"
timeline:
  start: "HH:MM"
  package_built: "HH:MM"
  total_minutes: 0
tasks:
  scaffold: pass # pass | fail
  custom_target: pass # pass | fail
  package: pass # pass | fail
issues:
  - severity: blocker # blocker | friction | suggestion
    step: "phase and command"
    quote: "verbatim user words, redacted"
    expected: "what the user expected"
    actual: "what happened"
    workaround: "none or description"
consent:
  recording: false
  anonymized_publication: true
notes: "facilitator observations, max 10 lines"
```

Reports carry IDs only, never names / companies / email addresses; replace usernames, hostnames, and tokens in paths with `<redacted>`. Delete raw material per the retention period in §3.

## 8. Externality and the T-M9-49 boundary

- This page delivers T-M9-48 (protocol, consent, script, severity, and report template); it does not claim an internal rehearsal or a real trial.
- T-M9-49 (≥ 3 human external developer trials) is an external / human activity; it cannot run in CI, in a script, or as an agent simulation.
- Never substitute an internal rehearsal, an AI role-play, or a placeholder survey for real reports; without signed consent and anonymized reports, T-M9-49 must not be marked complete.
- Coordinator write-back: link the anonymized reports, file issues for `blocker` findings, and update GA §7.2; keep [ENV-SLOTS](ENV-SLOTS.en.md) ENV-19 `OPEN` until 3 real reports exist.
