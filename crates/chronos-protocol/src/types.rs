//! NodeId, Term, Index, RequestId, Timestamp(u64 ns), IoId, TimerId.
//!
//! Spec: `docs/02-architecture.md` § Time, entropy, identity.
//! `IoId.local` resets on Recover. `IoId.incarnation` never goes backward.

/// Virtual time in nanoseconds, origin 0. Never a wall clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(pub u8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Term(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Index(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Get { key: Vec<u8> },
    Put { key: Vec<u8>, value: Vec<u8> },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimerId(pub u64);

/// Assigned by the harness at enqueue, not on `Effect::Send`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MsgId(pub u64);

/// Identifies in-flight I/O. Completions with a stale incarnation are ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IoId {
    pub incarnation: u64,
    pub local: u64,
}

#[cfg(test)]
mod tests {
    use super::IoId;

    #[test]
    fn io_id_is_eq_and_ord() {
        fn assert_eq_ord<T: Eq + Ord>() {}
        assert_eq_ord::<IoId>();

        let a = IoId {
            incarnation: 1,
            local: 0,
        };
        let b = IoId {
            incarnation: 1,
            local: 1,
        };
        let c = IoId {
            incarnation: 2,
            local: 0,
        };
        assert!(a < b);
        assert!(b < c);
        assert_eq!(a, a);
    }
}
