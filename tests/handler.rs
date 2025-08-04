#![allow(missing_docs, clippy::expect_used, unsafe_code)]

use {
    rinf::{DartSignal, RustSignal},
    rinf_router::Router,
    serde::{Deserialize, Serialize},
    serial_test::serial,
    std::{sync::OnceLock, time::Duration},
    tokio::{sync::mpsc::UnboundedSender, time::timeout},
};

static ACK_TX: OnceLock<UnboundedSender<()>> = OnceLock::new();

#[tokio::test]
#[serial]
async fn handler_return_value_is_forwarded() {
    let (ack_tx, mut ack_rx) = tokio::sync::mpsc::unbounded_channel();
    ACK_TX.set(ack_tx).expect("set global ACK_TX once");

    tokio::spawn(Router::new().route(echo_handler).run());
    send_test_signal(TestSignal);

    timeout(Duration::from_secs(10), async { ack_rx.recv().await })
        .await
        .expect("handler’s `send_signal_to_dart` was not called");
}

async fn echo_handler(_sig: TestSignal) -> (AckSignal,) {
    (AckSignal,)
}
#[derive(Debug, Serialize, Deserialize, DartSignal)]
struct TestSignal;

#[derive(Serialize)]
struct AckSignal;

impl RustSignal for AckSignal {
    fn send_signal_to_dart(&self) {
        if let Some(tx) = ACK_TX.get() {
            let _ = tx.send(());
        }
    }
}
fn send_test_signal(signal: TestSignal) {
    let msg = rinf::serialize(&signal).expect("postcard serialize");
    let bin: &[u8] = &[];

    unsafe {
        rinf_send_dart_signal_test_signal(msg.as_ptr(), msg.len(), bin.as_ptr(), bin.len());
    }
}
