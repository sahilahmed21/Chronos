# TDD evidence: P9 core-review coverage n_other

> **Superseded for runtime proof (2026-08-24):** tests are green under Linux CI and Docker `rust:1-bookworm`. Linker / “not a git repo” notes below are session archaeology.

## Source plan

Core review Minor: `coverage_flags` silently dropped `n ∉ {3,5}`. No `*.plan.md`.

Workspace was not a git repository during this TDD session — checkpoint commits skipped.


## User journeys

1. As an operator reading `--coverage`, I want unusual `n` values counted as `n_other`, so the batch table does not pretend those runs had no size flag.

## Task report

### Emit `n_other`

- **Summary:** `coverage_flags` pushes `n_other` when `n` is not 3 or 5.
- **Validation:** `cargo check -p chronos-sim --tests`; runtime `cargo test` blocked (`link.exe` missing).
- **RED:** Test `coverage_flags_reports_n_other_when_not_3_or_5` added first. Intended runtime failure: empty flags / missing `n_other`. Actual gate: linker not found (could not execute assertion RED).
- **GREEN (compile):** `cargo check -p chronos-sim --tests` and `cargo clippy -p chronos-sim --all-targets -- -D warnings` finish after `_ => flags.push("n_other")`.
- **Guarantee if tests run:** `Coverage { n: 7, … }` → `["n_other"]`; aggregate singles include `n_other: 1`.

## Test specification

| # | Guarantee | Test | Type | Result | Evidence |
|---|-----------|------|------|--------|----------|
| 1 | Non-3/5 `n` is flagged `n_other` | `fuzz.rs::coverage_flags_reports_n_other_when_not_3_or_5` | unit | COMPILE-ONLY | `cargo check -p chronos-sim --tests`; runtime blocked by linker |

## Coverage and gaps

- Runtime RED/GREEN not observed on this machine.
- Swarm today only plans `n ∈ {3,5}`; `n_other` is defensive for hand-built / future cfg.

## Merge evidence

N/A (no git).
