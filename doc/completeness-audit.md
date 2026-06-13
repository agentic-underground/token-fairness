# Completeness audit — token-fairness vs. the Original Ask

A rigorous check that **nothing from the original scheduler intent is missing or regressed** in the
`tf` port. Source docs audited (all under `doc/original-docs/`): `REVIEW_TOKEN_GUARD_FAILURE.md` (the
post-mortem), `token-aware-scheduling.md` (the operating model), `SKILL.md`, `schedule.md`.

**Verdict: nothing absent.** Every rule, layer, verdict, and mechanism is present as a `tf` verb, a
hook, or documented discipline. There is **one intentional, documented divergence** (guarded autonomous
resume) and three **already-documented accepted trade-offs**. No silent failures.

## Rule / mechanism coverage

| Item (original intent) | Status | Evidence in token-fairness |
|---|---|---|
| REVIEW Rule 1 — crons MONITOR/ALERT, never auto-resume | **DIVERGENT** (superseded) | See "The one divergence" below. `tf run-offpeak` resumes *under guards*; alerting is `startup-report.sh` + `tf report`. |
| REVIEW Rule 2 — budget-as-consent (`+Xk`, refuse without) | PRESENT | Profiles `budget_directive_required`; enforced by skill discipline + the harness Workflow `budget` API (not the language-neutral binary). |
| REVIEW Rule 3 — rate-limit error = HALT, do not retry | PRESENT | `ceiling.rs` HALT exit 10 / NO_SIGNAL exit 20; `gate` HALT/ASK; SKILL §4. |
| REVIEW Rule 4 — pre-flight > post-hoc | PRESENT | `tf preflight` (CONTINUE/PROBE, exit 0/3), `tf estimate`. |
| REVIEW Rule 5 — prefer serial for credit-sensitive | PRESENT (discipline) | SKILL / REVIEW lessons; advisory, not a guard. |
| L0 pre-flight estimate | PRESENT | `tf estimate` + `tf calibrate`. |
| L1 live ceiling guard | PRESENT | `tf ceiling-check` / `tf gate` (live `.rate_limits`, fail-closed). |
| L2 hard budget cap | PRESENT | profile flag + Workflow budget API (harness). |
| L3 off-peak scheduler | PRESENT | `tf offpeak-window` / `tf offpeak-budget`. |
| L4 cheap resume | PRESENT | `tf ledger {init,mark-done,mark-failed,remaining,pause,resume,set-offpeak,set-pointer,status}`. |
| Five verdicts CONTINUE/PROBE/DEFER/HALT/ASK (exit 0/3/4/10/20) | PRESENT | `gate`/`preflight`/`ceiling-check`. |
| Live→disk bridge + freshness fail-closed (`--snapshot-max-age` 900) | PRESENT | `tf snapshot`; `gate` snapshot fallback (stale → ASK). |
| Off-peak morning reserve | PRESENT | `tf offpeak-budget --morning-reserve` (login window held to `100−reserve`). |
| Durable + session-safe jobs (project + machine registry; prompt file; SessionStart re-arm) | PRESENT | `tf registry` (dual scope, `armed_via`); `startup-report.sh`; `run-offpeak` reads `.i2p/scheduled-jobs/<id>.prompt.txt`. |
| Convergence report ("how's the estimator doing?") | PRESENT | `tf report --estimator` (tiers SEEDING→CALIBRATING→CONVERGING→CONVERGED). |
| Non-negotiables (live not proxy; fail closed; throttle; budget=consent; cheap HALT) | PRESENT | across `snapshot`/`ceiling`/`gate`/profiles/`ledger`. |
| Profiles + `.i2p/job-profiles/` overrides | PRESENT | `profiles/*.json` (4 shipped); `state::resolve_profile`. |

## The one divergence (intentional, documented)

**Guarded autonomous off-peak resume.** REVIEW Rule 1 forbade crons from ever auto-resuming. That was an
**over-correction to a *broken* guard** (which watched a stale, wrong-metric `session.json` proxy and
never fired). `token-aware-scheduling.md` §Reconciliation supersedes it: with the live ceiling checked
per wave (L1, fail-closed), a hard budget cap (L2), and cheap incremental resume (L4), a **guarded**
scheduled job MAY resume autonomously off-peak when **all four** hold — (1) user consent, (2) `+Xk`
budget, (3) every wave through `tf gate`, (4) resume only from the ledger's `remaining`. A **blind**
cron (no guard, no budget) stays forbidden. `tf run-offpeak` implements exactly this guarded path
(flock single-instance → off-peak gate → headless `claude -p` whose prompt carries the budget/branch/
throttle rules). This is a deliberate restoration of the user's desired capability, not a regression.

## Accepted trade-offs (already documented in `doc/cutover.md`)

1. **Manual install, no i2p fallback.** Claude Code has no cross-marketplace dependency resolution, so
   the guard is a property of installing token-fairness. The i2p bash was retired (clean break).
2. **Per-arch binaries.** Only the host arch is committed; CI cross-compiles the rest (follow-up W2).
3. **Monthly USD cap not machine-readable.** Honestly disclosed; guarded by the `+Xk` budget + consent,
   never sensed. The 5-hour/7-day windows *are* live-guarded.

## No silent failures

`plan-close` emits a visible warning when `baseline==current==0` (the session-token writer missing);
`run-offpeak` logs and exits gracefully when the prompt file is absent; a stale snapshot yields ASK,
never a silent CONTINUE.
