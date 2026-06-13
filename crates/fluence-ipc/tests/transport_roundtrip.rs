// SPDX-License-Identifier: Apache-2.0

//! T1/T2 — the transport carries typed messages faithfully on this OS
//! (the CI matrix covers both platforms), survives fragmentation-sized
//! payloads, rejects oversized frames, and reports clean EOF as `None`.

use fluence_ipc::{HubToWorker, IpcEndpoint, MAX_FRAME_BYTES, WorkerToHub, connect, listen};
use proptest::prelude::*;

#[tokio::test]
async fn hello_handshake_round_trips() {
    let endpoint = IpcEndpoint::unique("test-hello");
    let mut listener = listen(&endpoint).await.expect("listen");

    let client_task = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            let mut worker = connect(&endpoint).await.expect("connect");
            let hello: HubToWorker = worker.recv().await.expect("recv").expect("not EOF");
            assert_eq!(hello, HubToWorker::Hello { v: 0 });
            worker
                .send(&WorkerToHub::HelloAck {
                    v: 0,
                    kind: "echo".into(),
                    pid: 42,
                })
                .await
                .expect("send ack");
        }
    });

    let mut hub_side = listener.accept().await.expect("accept");
    hub_side
        .send(&HubToWorker::Hello { v: 0 })
        .await
        .expect("send hello");
    let ack: WorkerToHub = hub_side.recv().await.expect("recv").expect("not EOF");
    assert_eq!(
        ack,
        WorkerToHub::HelloAck {
            v: 0,
            kind: "echo".into(),
            pid: 42
        }
    );
    client_task.await.expect("client task");
}

#[tokio::test]
async fn many_messages_keep_frame_boundaries() {
    let endpoint = IpcEndpoint::unique("test-many");
    let mut listener = listen(&endpoint).await.expect("listen");

    let client_task = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            let mut conn = connect(&endpoint).await.expect("connect");
            for seq in 0..100u64 {
                conn.send(&HubToWorker::Ping { seq }).await.expect("send");
            }
        }
    });

    let mut server = listener.accept().await.expect("accept");
    for expected in 0..100u64 {
        let message: HubToWorker = server.recv().await.expect("recv").expect("not EOF");
        assert_eq!(message, HubToWorker::Ping { seq: expected });
    }
    client_task.await.expect("client task");
}

#[tokio::test]
async fn clean_eof_is_none_not_error() {
    let endpoint = IpcEndpoint::unique("test-eof");
    let mut listener = listen(&endpoint).await.expect("listen");

    let client_task = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            // Connect and close immediately without sending anything.
            drop(connect(&endpoint).await.expect("connect"));
        }
    });

    let mut server = listener.accept().await.expect("accept");
    let eof: Option<HubToWorker> = server.recv().await.expect("clean close is not an error");
    assert!(eof.is_none());
    client_task.await.expect("client task");
}

#[tokio::test]
async fn large_payload_within_cap_round_trips() {
    let endpoint = IpcEndpoint::unique("test-large");
    let mut listener = listen(&endpoint).await.expect("listen");
    // 1 MiB payload: large enough to fragment across transport buffers.
    let payload = "x".repeat(1024 * 1024);

    let client_task = tokio::spawn({
        let endpoint = endpoint.clone();
        let payload = payload.clone();
        async move {
            let mut conn = connect(&endpoint).await.expect("connect");
            conn.send(&HubToWorker::Echo { payload })
                .await
                .expect("send");
        }
    });

    let mut server = listener.accept().await.expect("accept");
    let message: HubToWorker = server.recv().await.expect("recv").expect("not EOF");
    let HubToWorker::Echo { payload: received } = message else {
        panic!("expected echo");
    };
    assert_eq!(received.len(), payload.len());
    client_task.await.expect("client task");
}

#[tokio::test]
async fn oversized_frame_is_rejected_not_buffered() {
    let endpoint = IpcEndpoint::unique("test-oversize");
    let mut listener = listen(&endpoint).await.expect("listen");

    let client_task = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            let mut conn = connect(&endpoint).await.expect("connect");
            // The send side enforces the cap too: sending must fail.
            let oversized = HubToWorker::Echo {
                payload: "x".repeat(MAX_FRAME_BYTES + 1),
            };
            let error = conn
                .send(&oversized)
                .await
                .expect_err("oversized must fail");
            let message = error.to_string();
            assert!(message.contains("transport"), "unexpected error: {message}");
        }
    });

    let _server = listener.accept().await.expect("accept");
    client_task.await.expect("client task");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, ..ProptestConfig::default() })]
    /// Any payload (unicode included) survives the wire bit-identically.
    #[test]
    fn arbitrary_payloads_round_trip(payload in "\\PC{0,2048}") {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let endpoint = IpcEndpoint::unique("test-prop");
            let mut listener = listen(&endpoint).await.expect("listen");
            let sent = payload.clone();
            let client = tokio::spawn(async move {
                let mut conn = connect(&endpoint).await.expect("connect");
                conn.send(&HubToWorker::Echo { payload: sent }).await.expect("send");
            });
            let mut server = listener.accept().await.expect("accept");
            let received: HubToWorker = server.recv().await.expect("recv").expect("not EOF");
            assert_eq!(received, HubToWorker::Echo { payload });
            client.await.expect("client");
        });
    }
}
