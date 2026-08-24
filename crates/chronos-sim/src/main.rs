//! Headless swarm: `--seed`, `--seeds`, `--replay`, `--minify`. `run()` stays the G3 oracle.
//!
//! Fail files are written here, not inside the Cluster. `--replay` verifies
//! digest, check, config, and extras. `--minify` delta-debugs extras + delivery
//! tokens. `--coverage` folds observed fault flags after a batch.
//! Spec: `docs/roadmap/P07-bug.md`, `docs/roadmap/P08-minify.md`, `docs/roadmap/P09-product.md`.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chronos_sim::{
    aggregate_coverage, digest_hex, fail_file_header, format_coverage_table, format_fail_file,
    format_min_schedule, format_planned_schedule, format_replay_line, minify, run_seed,
    verify_replay, Coverage, MinResult, MinifyOutcome, RunReport,
};

fn main() -> ExitCode {
    match parse_args(env::args().skip(1)) {
        Ok(cmd) => run_cmd(cmd),
        Err(msg) => {
            let _ = writeln!(io::stderr(), "{msg}");
            let _ = writeln!(io::stderr(), "{USAGE}");
            ExitCode::from(2)
        }
    }
}

const USAGE: &str = "\
chronos-sim --seed <u64> [--out <dir>]
chronos-sim --seeds <n> [--start <u64>] [--out <dir>] [--coverage]
chronos-sim --replay <file>
chronos-sim --minify --seed <u64> [--out <dir>]

P7 replay prints REPRODUCED / CLEAN / DID_NOT_REPRODUCE / MISMATCH (exit 2 on mismatch).
P8 minify prints MINIFIED (exit 1, including capped) / MINIFY CLEAN (exit 0). MINIFY MISMATCH (exit 2) is only a full-schedule miss.
P9: any CheckFail (including abort) exits 1 on --seed/--seeds. PR swarm N=32 --start 1; nightly N=1000. 10k is a stretch goal.";

enum Cmd {
    One {
        seed: u64,
        out: PathBuf,
    },
    Batch {
        start: u64,
        count: u64,
        out: PathBuf,
        coverage: bool,
    },
    Replay {
        path: PathBuf,
    },
    Minify {
        seed: u64,
        out: PathBuf,
    },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cmd, String> {
    let mut seed = None;
    let mut seeds = None;
    let mut start = 0u64;
    let mut replay = None;
    let mut minify_mode = false;
    let mut coverage = false;
    let mut out = PathBuf::from(".");
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--seed" => {
                seed = Some(parse_u64(&next(&mut it, "--seed")?, "--seed")?);
            }
            "--seeds" => {
                seeds = Some(parse_u64(&next(&mut it, "--seeds")?, "--seeds")?);
            }
            "--start" => {
                start = parse_u64(&next(&mut it, "--start")?, "--start")?;
            }
            "--replay" => {
                replay = Some(PathBuf::from(next(&mut it, "--replay")?));
            }
            "--minify" => {
                minify_mode = true;
            }
            "--coverage" => {
                coverage = true;
            }
            "--out" => {
                out = PathBuf::from(next(&mut it, "--out")?);
            }
            "-h" | "--help" => return Err("usage".into()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if coverage && seeds.is_none() {
        return Err("--coverage requires --seeds".into());
    }
    if minify_mode {
        if seeds.is_some() || replay.is_some() || coverage {
            return Err("--minify cannot be combined with --seeds, --replay, or --coverage".into());
        }
        let seed = seed.ok_or_else(|| "--minify requires --seed".to_string())?;
        return Ok(Cmd::Minify { seed, out });
    }
    let n_modes = u8::from(seed.is_some()) + u8::from(seeds.is_some()) + u8::from(replay.is_some());
    if n_modes != 1 {
        return Err("exactly one of --seed, --seeds, --replay is required".into());
    }
    if let Some(seed) = seed {
        return Ok(Cmd::One { seed, out });
    }
    if let Some(count) = seeds {
        if count == 0 {
            return Err("--seeds must be greater than 0".into());
        }
        if start.checked_add(count - 1).is_none() {
            return Err("--start + --seeds overflows u64".into());
        }
        return Ok(Cmd::Batch {
            start,
            count,
            out,
            coverage,
        });
    }
    Ok(Cmd::Replay {
        path: replay.ok_or_else(|| "replay path missing".to_string())?,
    })
}

fn next(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_u64(s: &str, flag: &str) -> Result<u64, String> {
    s.parse()
        .map_err(|_| format!("{flag}: expected u64, got {s}"))
}

fn run_cmd(cmd: Cmd) -> ExitCode {
    match cmd {
        Cmd::One { seed, out } => {
            let report = run_seed(seed);
            print_report(&report);
            if let Err(e) = maybe_dump(&out, &report) {
                let _ = writeln!(io::stderr(), "{e}");
                return ExitCode::from(2);
            }
            if report.check.is_some() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Cmd::Batch {
            start,
            count,
            out,
            coverage,
        } => {
            let mut failed = 0u64;
            let mut counters: Vec<Coverage> = Vec::new();
            for i in 0..count {
                let seed = start.saturating_add(i);
                let report = run_seed(seed);
                print_report(&report);
                if coverage {
                    counters.push(report.counters.clone());
                }
                if let Err(e) = maybe_dump(&out, &report) {
                    let _ = writeln!(io::stderr(), "{e}");
                    return ExitCode::from(2);
                }
                if report.check.is_some() {
                    failed = failed.saturating_add(1);
                }
            }
            if coverage {
                print!("{}", format_coverage_table(&aggregate_coverage(&counters)));
            }
            if failed > 0 {
                let _ = writeln!(io::stderr(), "{failed}/{count} seeds failed");
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Cmd::Replay { path } => {
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    let _ = writeln!(io::stderr(), "read {}: {e}", path.display());
                    return ExitCode::from(2);
                }
            };
            let Some(header) = fail_file_header(&bytes) else {
                let _ = writeln!(io::stderr(), "not a chronos-fail file: {}", path.display());
                return ExitCode::from(2);
            };
            let report = run_seed(header.seed);
            let verdict = verify_replay(&header, &report);
            println!("{}", format_replay_line(verdict, &header, &report));
            ExitCode::from(verdict.exit_code())
        }
        Cmd::Minify { seed, out } => {
            let outcome = minify(seed);
            match &outcome {
                MinifyOutcome::Clean => {
                    println!("MINIFY CLEAN seed={seed}");
                    ExitCode::SUCCESS
                }
                MinifyOutcome::Abort { check } => {
                    let _ = writeln!(
                        io::stderr(),
                        "MINIFY ABORT seed={seed} check={}",
                        check.as_label()
                    );
                    ExitCode::from(2)
                }
                MinifyOutcome::HarnessMismatch => {
                    let _ = writeln!(
                        io::stderr(),
                        "MINIFY MISMATCH seed={seed} full schedule did not reproduce"
                    );
                    ExitCode::from(2)
                }
                MinifyOutcome::Minified(result) => {
                    println!(
                        "MINIFIED seed={} check={} atoms={}->{} extras={}->{} rounds={} capped={}",
                        result.seed,
                        result.check.as_label(),
                        result.atoms_before,
                        result.atoms_after,
                        result.extras_before,
                        result.extras_after,
                        result.rounds,
                        u8::from(result.capped)
                    );
                    if let Err(e) = write_min_schedule(&out, result) {
                        let _ = writeln!(io::stderr(), "{e}");
                        return ExitCode::from(2);
                    }
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn print_report(report: &RunReport) {
    let digest = digest_hex(&report.digest);
    let c = &report.counters;
    match &report.check {
        Some(fail) => {
            let kind = if fail.check.is_abort() {
                "ABORT"
            } else {
                "FAIL"
            };
            println!(
                "{kind} seed={} digest={digest} check={} detail={} profile={} n={}",
                report.seed,
                fail.check.as_label(),
                fail.detail.replace(['\n', '\r'], " "),
                report.profile.as_str(),
                report.cfg.n
            );
        }
        None => {
            println!(
                "ok seed={} digest={digest} profile={} n={} crash={} partition={} fsync_err={} drop={} dup={} buggify={}",
                report.seed,
                report.profile.as_str(),
                c.n,
                u8::from(c.crash),
                u8::from(c.partition),
                u8::from(c.fsync_err),
                u8::from(c.drop),
                u8::from(c.dup),
                u8::from(c.buggify)
            );
        }
    }
}

fn maybe_dump(out: &Path, report: &RunReport) -> io::Result<()> {
    if report.check.is_none() {
        return Ok(());
    }
    if !out.as_os_str().is_empty() && out != Path::new(".") && !out.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("--out {} does not exist", out.display()),
        ));
    }
    let path = out.join(format!("fail-{}.trace", report.seed));
    let planned = out.join(format!("fail-{}.planned", report.seed));
    let trace_bytes = format_fail_file(report);
    let planned_bytes = format_planned_schedule(&report.extras);
    fs::write(&path, &trace_bytes)?;
    if let Err(e) = fs::write(&planned, &planned_bytes) {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    println!("wrote {}", path.display());
    println!("wrote {}", planned.display());
    Ok(())
}

fn write_min_schedule(out: &Path, result: &MinResult) -> io::Result<()> {
    if !out.as_os_str().is_empty() && out != Path::new(".") && !out.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("--out {} does not exist", out.display()),
        ));
    }
    let path = out.join(format!("fail-{}.min", result.seed));
    fs::write(&path, format_min_schedule(result))?;
    println!("wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seed() {
        let cmd = parse_args(["--seed".into(), "42".into()]).unwrap();
        match cmd {
            Cmd::One { seed, out } => {
                assert_eq!(seed, 42);
                assert_eq!(out, PathBuf::from("."));
            }
            _ => panic!("expected One"),
        }
    }

    #[test]
    fn parse_seeds_start_out() {
        let cmd = parse_args([
            "--seeds".into(),
            "50".into(),
            "--start".into(),
            "10".into(),
            "--out".into(),
            "traces".into(),
        ])
        .unwrap();
        match cmd {
            Cmd::Batch {
                start,
                count,
                out,
                coverage,
            } => {
                assert_eq!(start, 10);
                assert_eq!(count, 50);
                assert_eq!(out, PathBuf::from("traces"));
                assert!(!coverage);
            }
            _ => panic!("expected Batch"),
        }
    }

    #[test]
    fn parse_seeds_coverage() {
        let cmd = parse_args([
            "--seeds".into(),
            "32".into(),
            "--start".into(),
            "1".into(),
            "--coverage".into(),
        ])
        .unwrap();
        match cmd {
            Cmd::Batch {
                start,
                count,
                coverage,
                ..
            } => {
                assert_eq!(start, 1);
                assert_eq!(count, 32);
                assert!(coverage);
            }
            _ => panic!("expected Batch"),
        }
    }

    #[test]
    fn parse_coverage_requires_seeds() {
        assert!(parse_args(["--coverage".into()]).is_err());
        assert!(parse_args(["--seed".into(), "1".into(), "--coverage".into()]).is_err());
    }

    #[test]
    fn parse_replay() {
        let cmd = parse_args(["--replay".into(), "fail-7.trace".into()]).unwrap();
        match cmd {
            Cmd::Replay { path, .. } => assert_eq!(path, PathBuf::from("fail-7.trace")),
            _ => panic!("expected Replay"),
        }
    }

    #[test]
    fn parse_rejects_two_modes() {
        assert!(parse_args(["--seed".into(), "1".into(), "--seeds".into(), "2".into()]).is_err());
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(parse_args(Vec::<String>::new()).is_err());
    }

    #[test]
    fn parse_rejects_zero_seeds() {
        assert!(parse_args(["--seeds".into(), "0".into()]).is_err());
    }

    #[test]
    fn parse_rejects_seed_range_overflow() {
        assert!(parse_args([
            "--seeds".into(),
            "2".into(),
            "--start".into(),
            u64::MAX.to_string(),
        ])
        .is_err());
    }

    #[test]
    fn parse_minify_seed() {
        let cmd = parse_args(["--minify".into(), "--seed".into(), "7".into()]).unwrap();
        match cmd {
            Cmd::Minify { seed, out } => {
                assert_eq!(seed, 7);
                assert_eq!(out, PathBuf::from("."));
            }
            _ => panic!("expected Minify"),
        }
    }

    #[test]
    fn parse_minify_requires_seed() {
        assert!(parse_args(["--minify".into()]).is_err());
    }

    #[test]
    fn parse_rejects_minify_with_replay() {
        assert!(parse_args([
            "--minify".into(),
            "--seed".into(),
            "1".into(),
            "--replay".into(),
            "fail.trace".into(),
        ])
        .is_err());
    }

    #[test]
    fn parse_rejects_minify_with_coverage() {
        assert!(parse_args([
            "--minify".into(),
            "--seed".into(),
            "1".into(),
            "--coverage".into(),
        ])
        .is_err());
    }
}
