//! `apply(entry) -> response`. Idempotency table is state-machine state (`BTreeMap`).
//!
//! Spec: `docs/02-architecture.md` D8. Gets are log entries in v1 (D7).

use std::collections::BTreeMap;

use crate::effect::{ClientError, ClientResp};
use crate::types::{ClientId, Cmd, RequestId};

pub fn apply(
    store: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    idempotency: &mut BTreeMap<(ClientId, RequestId), ClientResp>,
    client: ClientId,
    request: RequestId,
    cmd: &Cmd,
) -> ClientResp {
    if let Some(resp) = idempotency.get(&(client, request)) {
        return resp.clone();
    }
    let resp = match cmd {
        Cmd::Get { key } => match store.get(key) {
            Some(value) => ClientResp::Ok {
                value: value.clone(),
            },
            None => ClientResp::Err(ClientError::NotFound),
        },
        Cmd::Put { key, value } => {
            store.insert(key.clone(), value.clone());
            ClientResp::Ok {
                value: value.clone(),
            }
        }
    };
    idempotency.insert((client, request), resp.clone());
    resp
}
