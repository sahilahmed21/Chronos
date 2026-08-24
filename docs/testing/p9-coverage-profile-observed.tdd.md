# TDD evidence: P9 coverage profile vs observed

> **Superseded for runtime proof (2026-08-24):** tests are green under Linux CI and Docker `rust:1-bookworm`. Linker / “not a git repo” notes below are session archaeology.

## Source plan

Core review Major: `finish_report` / coverage table mixed cfg knobs (`brutal`, `buggify`, `n`) with observed fault fires under one “observed” claim.

Workspace was not a git repository during this TDD session — checkpoint commits skipped.


## User journeys

1. As an operator reading `--coverage`, I want profile knobs listed separately from faults that fired, so I do not treat “buggify on” as “buggify path executed.”

## Task report

### Split profile vs observed

- **Summary:** `coverage_profile_flags` / `coverage_observed_flags`; `CoverageSummary` has `profile` + `observed`; pairs only over observed; table labels both.
- **Validation:** `cargo check -p chronos-sim --tests`; `cargo clippy -p chronos-sim --all-targets -- -D warnings`. Runtime tests blocked (`link.exe`).
- **RED:** Not executed (linker). Intended failure before fix: `format_coverage_separates_profile_knobs_from_observed_faults` would see brutal under a single singles map / “these fired” claim.
- **GREEN (compile):** check + clippy after split.
- **Guarantee if tests run:** buggify+brutal with no crash → under `# profile` only; crash+partition pairs exclude profile keys.

## Test specification

| # | Guarantee | Test | Type | Result | Evidence |
|---|-----------|------|------|--------|----------|
| 1 | Profile knobs not in observed map | `aggregate_coverage_counts_singles_and_pairs` | unit | COMPILE-ONLY | cargo check |
| 2 | Table separates sections | `format_coverage_separates_profile_knobs_from_observed_faults` | unit | COMPILE-ONLY | cargo check |
| 3 | n_other stays profile | `coverage_flags_reports_n_other_when_not_3_or_5` | unit | COMPILE-ONLY | cargo check |

## Coverage and gaps

- Runtime RED/GREEN not observed on this machine.
- Skipped: git init / COMMIT / CI green (needs operator env).

## Merge evidence

N/A (no git).
