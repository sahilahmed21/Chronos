//! ADR-0002: the default build is byte-identical to pre-feature behavior, and
//! (with `--features mutants`) each deliberate bug is caught by the checkers.

use chronos_sim::{digest_hex, run_seed};

/// Pinned from the nightly CI log (seed 1, brutal, n=3) before the `mutants` feature existed.
const SEED1_DIGEST: &str = "91b52023e8fb85f224941d4c58d763479a4b8f217584cbdfeaa8af6a782ac2a3";

#[test]
fn seed_1_digest_is_unchanged_by_the_mutants_feature() {
    let r = run_seed(1);
    assert!(r.check.is_none());
    assert_eq!(digest_hex(&r.digest), SEED1_DIGEST);
}

#[cfg(not(feature = "mutants"))]
#[test]
#[should_panic(expected = "mutants")]
fn selecting_a_mutant_without_the_feature_panics() {
    let mut n = chronos_protocol::Node::new(chronos_protocol::NodeId(0), Vec::new());
    n.set_mutant(Some(chronos_protocol::Mutant::CommitOldTerm));
}

#[cfg(feature = "mutants")]
mod with_mutants {
    use chronos_protocol::Mutant;
    use chronos_sim::{run_seed, run_seed_mutant, CheckName};

    fn first_detection(m: Mutant, seeds: u64) -> Option<(u64, CheckName)> {
        (1..=seeds).find_map(|s| run_seed_mutant(s, m).check.map(|c| (s, c.check)))
    }

    #[test]
    fn clean_swarm_stays_clean_with_the_feature_compiled_in() {
        for s in 1..=300 {
            assert!(run_seed(s).check.is_none(), "seed {s}");
        }
    }

    #[test]
    fn mutant_names_round_trip() {
        for m in Mutant::ALL {
            assert_eq!(Mutant::from_name(m.name()), Some(m));
        }
    }

    #[test]
    fn ack_before_fsync_is_caught_as_persist_before_ack() {
        let (_, check) = first_detection(Mutant::AckBeforeFsync, 200).expect("undetected");
        assert_eq!(check, CheckName::PersistBeforeAck);
    }

    /// First detection seeds measured on 2026-11-10 (WSL2 Ubuntu, release build). See README.
    #[test]
    fn pinned_first_detection_seeds_still_fail_with_the_expected_check() {
        let pinned = [
            (Mutant::SkipVotePersist, 0, CheckName::PersistBeforeSend),
            (Mutant::AckBeforeFsync, 0, CheckName::PersistBeforeAck),
            (Mutant::CommitOldTerm, 227, CheckName::CurrentTermCommit),
            (Mutant::GetFromMemory, 148, CheckName::Linearizability),
            (Mutant::NoLeaderNoop, 5287, CheckName::Linearizability),
            (Mutant::DropIdempotency, 28061, CheckName::Linearizability),
        ];
        for (m, seed, want) in pinned {
            let got = run_seed_mutant(seed, m).check.map(|c| c.check);
            assert_eq!(got, Some(want), "{} seed {seed}", m.name());
        }
    }
}
