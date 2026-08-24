//! Client invoke/ok/fail history for linearizability (Porcupine-style).
//!
//! Ops are keyed by `(client, request)`. Retries are the same op. Incomplete
//! ops (no Reply) are allowed. Happens-before is `(time, seq)`.
//! Spec: `docs/02-architecture.md` § Black-box (KV).

use std::collections::{BTreeMap, BTreeSet};

use chronos_protocol::{ClientError, ClientId, ClientReq, ClientResp, Cmd, RequestId, Timestamp};

use crate::check::{CheckFail, CheckName};

const MAX_OPS: usize = 48;

type Stamp = (Timestamp, u64);
type LinState = BTreeMap<Vec<u8>, Vec<u8>>;
type LinCache = BTreeSet<(u64, LinState)>;

#[derive(Clone, Debug)]
struct HistoryOp {
    cmd: Cmd,
    invoke: Stamp,
    complete: Option<Stamp>,
    result: Option<ClientResp>,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    ops: BTreeMap<(ClientId, RequestId), HistoryOp>,
    order: Vec<(ClientId, RequestId)>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invoke(&mut self, at: Timestamp, seq: u64, req: &ClientReq) {
        let key = (req.client, req.request);
        if self.ops.contains_key(&key) {
            return;
        }
        self.order.push(key);
        self.ops.insert(
            key,
            HistoryOp {
                cmd: req.cmd.clone(),
                invoke: (at, seq),
                complete: None,
                result: None,
            },
        );
    }

    /// Complete the op keyed by `(client, request)`. Extra replies for the same
    /// request are ignored. Unmatched Reply is a recorder error.
    pub fn complete(
        &mut self,
        at: Timestamp,
        seq: u64,
        client: ClientId,
        request: RequestId,
        resp: ClientResp,
    ) -> Result<(), CheckFail> {
        let key = (client, request);
        let Some(op) = self.ops.get_mut(&key) else {
            return Err(CheckFail::new(
                CheckName::UnmatchedReply,
                format!("Reply for {client:?} {request:?} with no invoke"),
            ));
        };
        if op.complete.is_some() {
            return Ok(());
        }
        op.complete = Some((at, seq));
        op.result = Some(resp);
        Ok(())
    }

    pub fn linearizable(&self) -> Result<(), CheckFail> {
        if self.ops.len() > MAX_OPS {
            return Err(CheckFail::new(
                CheckName::CheckerCapacity,
                format!("history has {} ops, cap {MAX_OPS}", self.ops.len()),
            ));
        }
        let ops: Vec<&HistoryOp> = self.order.iter().filter_map(|k| self.ops.get(k)).collect();
        let n = ops.len();
        if n == 0 {
            return Ok(());
        }
        let mut preds = vec![0u64; n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                if let Some(ci) = ops[i].complete {
                    if ci < ops[j].invoke {
                        preds[j] |= 1u64 << i;
                    }
                }
            }
        }
        let remaining = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        let mut cache = LinCache::new();
        if search(&LinState::new(), remaining, &ops, &preds, &mut cache) {
            Ok(())
        } else {
            Err(CheckFail::new(
                CheckName::Linearizability,
                "no linearization of the client history",
            ))
        }
    }
}

fn spec_apply(map: &mut LinState, cmd: &Cmd) -> ClientResp {
    match cmd {
        Cmd::Get { key } => match map.get(key) {
            Some(value) => ClientResp::Ok {
                value: value.clone(),
            },
            None => ClientResp::Err(ClientError::NotFound),
        },
        Cmd::Put { key, value } => {
            map.insert(key.clone(), value.clone());
            ClientResp::Ok {
                value: value.clone(),
            }
        }
    }
}

fn is_definite_fail(resp: &ClientResp) -> bool {
    matches!(
        resp,
        ClientResp::Err(ClientError::Io | ClientError::Invalid | ClientError::NotLeader)
    )
}

fn search(
    state: &LinState,
    remaining: u64,
    ops: &[&HistoryOp],
    preds: &[u64],
    cache: &mut LinCache,
) -> bool {
    if remaining == 0 {
        return true;
    }
    if !cache.insert((remaining, state.clone())) {
        return false;
    }
    let n = ops.len();
    for i in 0..n {
        let bit = 1u64 << i;
        if remaining & bit == 0 {
            continue;
        }
        if preds[i] & remaining != 0 {
            continue;
        }
        let rest = remaining ^ bit;
        match ops[i].result.as_ref() {
            Some(resp) if is_definite_fail(resp) => {
                if search(state, rest, ops, preds, cache) {
                    return true;
                }
            }
            Some(resp) => {
                let mut next = state.clone();
                let got = spec_apply(&mut next, &ops[i].cmd);
                if &got == resp && search(&next, rest, ops, preds, cache) {
                    return true;
                }
            }
            None => {
                if search(state, rest, ops, preds, cache) {
                    return true;
                }
                let mut next = state.clone();
                spec_apply(&mut next, &ops[i].cmd);
                if search(&next, rest, ops, preds, cache) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_protocol::{ClientId, RequestId};

    fn put_req(request: u64, value: &[u8]) -> ClientReq {
        ClientReq {
            client: ClientId(1),
            request: RequestId(request),
            cmd: Cmd::Put {
                key: b"k".to_vec(),
                value: value.to_vec(),
            },
        }
    }

    fn get_req(client: u64, request: u64) -> ClientReq {
        ClientReq {
            client: ClientId(client),
            request: RequestId(request),
            cmd: Cmd::Get { key: b"k".to_vec() },
        }
    }

    fn ok(value: &[u8]) -> ClientResp {
        ClientResp::Ok {
            value: value.to_vec(),
        }
    }

    #[test]
    fn put_then_get_is_linearizable() {
        let mut h = History::new();
        h.invoke(Timestamp(1), 0, &put_req(1, b"v"));
        h.complete(Timestamp(2), 0, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        h.invoke(Timestamp(3), 0, &get_req(1, 2));
        h.complete(Timestamp(4), 0, ClientId(1), RequestId(2), ok(b"v"))
            .unwrap();
        assert!(h.linearizable().is_ok());
    }

    #[test]
    fn get_before_put_in_real_time_is_not_linearizable() {
        let mut h = History::new();
        h.invoke(Timestamp(1), 0, &get_req(1, 1));
        h.complete(Timestamp(2), 0, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        h.invoke(Timestamp(3), 0, &put_req(2, b"v"));
        h.complete(Timestamp(4), 0, ClientId(1), RequestId(2), ok(b"v"))
            .unwrap();
        assert!(h.linearizable().is_err());
    }

    #[test]
    fn incomplete_ops_do_not_crash_the_checker() {
        let mut h = History::new();
        h.invoke(Timestamp(1), 0, &put_req(1, b"v"));
        assert!(h.linearizable().is_ok());
    }

    #[test]
    fn retry_same_ids_is_one_op() {
        let mut h = History::new();
        let req = put_req(1, b"v");
        h.invoke(Timestamp(1), 0, &req);
        h.invoke(Timestamp(2), 0, &req);
        h.complete(Timestamp(3), 0, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        assert_eq!(h.ops.len(), 1);
        assert!(h.linearizable().is_ok());
    }

    #[test]
    fn not_leader_does_not_close_an_earlier_in_flight_op() {
        let mut h = History::new();
        h.invoke(Timestamp(1), 1, &put_req(1, b"a"));
        h.invoke(Timestamp(1), 2, &put_req(2, b"b"));
        h.complete(
            Timestamp(1),
            2,
            ClientId(1),
            RequestId(2),
            ClientResp::Err(ClientError::NotLeader),
        )
        .unwrap();
        assert!(h.ops[&(ClientId(1), RequestId(1))].complete.is_none());
        assert!(h.ops[&(ClientId(1), RequestId(2))].complete.is_some());
    }

    #[test]
    fn unmatched_reply_is_a_recorder_error() {
        let mut h = History::new();
        let err = h
            .complete(Timestamp(1), 0, ClientId(1), RequestId(9), ok(b"v"))
            .unwrap_err();
        assert_eq!(err.check, CheckName::UnmatchedReply);
    }

    #[test]
    fn extra_reply_for_the_same_request_is_ignored() {
        let mut h = History::new();
        h.invoke(Timestamp(1), 0, &put_req(1, b"v"));
        h.complete(Timestamp(2), 0, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        h.complete(Timestamp(3), 0, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        assert_eq!(
            h.ops[&(ClientId(1), RequestId(1))].complete,
            Some((Timestamp(2), 0))
        );
    }

    #[test]
    fn same_timestamp_seq_orders_happens_before() {
        let mut h = History::new();
        h.invoke(Timestamp(10), 0, &put_req(1, b"v"));
        h.complete(Timestamp(10), 1, ClientId(1), RequestId(1), ok(b"v"))
            .unwrap();
        h.invoke(Timestamp(10), 2, &get_req(1, 2));
        h.complete(
            Timestamp(10),
            3,
            ClientId(1),
            RequestId(2),
            ClientResp::Err(ClientError::NotFound),
        )
        .unwrap();
        assert!(h.linearizable().is_err());
    }

    #[test]
    fn over_capacity_is_not_named_linearizability() {
        let mut h = History::new();
        for i in 0..=MAX_OPS as u64 {
            h.invoke(Timestamp(i), i, &put_req(i, b"v"));
        }
        let err = h.linearizable().unwrap_err();
        assert_eq!(err.check, CheckName::CheckerCapacity);
    }
}
