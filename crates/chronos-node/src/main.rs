//! Production node. Same `Effect`s as sim, real files / SystemTime later.
//!
//! P1: single-node WAL KV. No TCP. Spec: `docs/02-architecture.md`.

mod disk;
mod net;
mod timer;

use std::collections::VecDeque;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use chronos_protocol::{
    ClientId, ClientReq, ClientResp, Cmd, Effect, Event, Node, NodeId, RequestId, TIMER_ELECTION,
};

use disk::FileDisk;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr(), "{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("chronos.wal"));

    let mut disk = FileDisk::open(&path)?;
    let durable = disk.load_and_truncate()?;
    let mut node = Node::new(NodeId(0), Vec::new());
    drive(&mut node, &mut disk, Event::Recover { durable });
    drive(
        &mut node,
        &mut disk,
        Event::TimerFired {
            timer: TIMER_ELECTION,
        },
    );

    let put = Event::ClientRequest {
        req: ClientReq {
            client: ClientId(1),
            request: RequestId(1),
            cmd: Cmd::Put {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            },
        },
    };
    let put_replies = drive(&mut node, &mut disk, put);
    let get = Event::ClientRequest {
        req: ClientReq {
            client: ClientId(1),
            request: RequestId(2),
            cmd: Cmd::Get { key: b"k".to_vec() },
        },
    };
    let get_replies = drive(&mut node, &mut disk, get);

    println!("put={put_replies:?}");
    println!("get={get_replies:?}");
    Ok(())
}

fn drive(node: &mut Node, disk: &mut FileDisk, event: Event) -> Vec<(ClientId, ClientResp)> {
    let mut replies = Vec::new();
    let mut q = VecDeque::new();
    q.push_back(event);
    while let Some(ev) = q.pop_front() {
        let effects = node.step(ev);
        let completions = disk.submit(&effects);
        for effect in &effects {
            if let Effect::Reply { to, resp, .. } = effect {
                replies.push((*to, resp.clone()));
            }
        }
        q.extend(completions);
    }
    replies
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_protocol::ClientResp;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_WAL: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn put_get_via_file_disk() {
        let n = TEST_WAL.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("chronos-p1-{}-{n}.wal", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut disk = FileDisk::open(&path).expect("open wal");
        let durable = disk.load_and_truncate().expect("load");
        let mut node = Node::new(NodeId(0), Vec::new());
        drive(&mut node, &mut disk, Event::Recover { durable });
        drive(
            &mut node,
            &mut disk,
            Event::TimerFired {
                timer: TIMER_ELECTION,
            },
        );

        let put_replies = drive(
            &mut node,
            &mut disk,
            Event::ClientRequest {
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(1),
                    cmd: Cmd::Put {
                        key: b"k".to_vec(),
                        value: b"v".to_vec(),
                    },
                },
            },
        );

        assert_eq!(
            put_replies,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );

        let get_replies = drive(
            &mut node,
            &mut disk,
            Event::ClientRequest {
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(2),
                    cmd: Cmd::Get { key: b"k".to_vec() },
                },
            },
        );

        assert_eq!(
            get_replies,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );

        let _ = std::fs::remove_file(&path);
    }
}
