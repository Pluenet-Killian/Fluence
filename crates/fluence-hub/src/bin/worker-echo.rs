// SPDX-License-Identifier: Apache-2.0

//! Echo test worker (PLAN 2.2): connects back over IPC, answers the
//! handshake and heartbeats, mirrors `Echo` payloads, exits politely on
//! `Shutdown`. The kill-tests murder it in every way a real inference
//! worker could die.

use std::process::ExitCode;

use fluence_ipc::{HubToWorker, IPC_PROTOCOL_VERSION, IpcEndpoint, WorkerToHub};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let Some(path) = std::env::args().skip_while(|arg| arg != "--ipc").nth(1) else {
        eprintln!("worker-echo: missing --ipc <endpoint>");
        return ExitCode::FAILURE;
    };
    let endpoint = IpcEndpoint::from_path(path);
    // Kill-test support: when the (test-set) variable is inherited from
    // the hub, record our pid there so tests can target us precisely.
    // Absent in production — zero effect.
    if let Ok(pid_dir) = std::env::var("FLUENCE_PID_DIR") {
        let path =
            std::path::Path::new(&pid_dir).join(format!("worker-echo-{}.pid", std::process::id()));
        if std::fs::write(path, std::process::id().to_string()).is_err() {
            eprintln!("worker-echo: cannot write pid file");
        }
    }

    let mut connection = match fluence_ipc::connect(&endpoint).await {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("worker-echo: connect failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    loop {
        match connection.recv::<HubToWorker>().await {
            Ok(Some(HubToWorker::Hello { v })) => {
                if v != IPC_PROTOCOL_VERSION {
                    eprintln!(
                        "worker-echo: protocol mismatch (hub {v}, worker {IPC_PROTOCOL_VERSION})"
                    );
                    return ExitCode::FAILURE;
                }
                let ack = WorkerToHub::HelloAck {
                    v: IPC_PROTOCOL_VERSION,
                    kind: "echo".to_owned(),
                    pid: std::process::id(),
                };
                if connection.send(&ack).await.is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Ok(Some(HubToWorker::Ping { seq })) => {
                if connection.send(&WorkerToHub::Pong { seq }).await.is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Ok(Some(HubToWorker::Echo { payload })) => {
                if connection
                    .send(&WorkerToHub::EchoReply { payload })
                    .await
                    .is_err()
                {
                    return ExitCode::FAILURE;
                }
            }
            Ok(Some(HubToWorker::Shutdown) | None) => return ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("worker-echo: ipc error: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
}
