//! Test utilities for middleware and handler testing.
//!
//! This module is only available when the `test-helpers` feature is enabled.
//! It provides reusable testing utilities for middleware composition and
//! execution order validation.

use {
    crate::{handler::Handler, into_response::IntoResponse},
    futures::future::BoxFuture,
    rinf::{DartSignal, RustSignal},
    std::future::Future,
    tower::{Layer, Service},
};

/// Verifies a [`Handler`]
pub fn assert_handler<T, S, H: Handler<T, S>>(handler: H) -> H {
    handler
}

/// A reusable test signal type with a message String field.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, rinf::DartSignal, rinf::RustSignal,
)]
pub struct Signal {
    /// The message content of the signal
    pub message: String,
}

impl Signal {
    /// Create a new TestSignal with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc, unsafe_code)]
/// Send a test signal via FFI for testing purposes
pub fn send_signal(signal: Signal) {
    let msg = rinf::serialize(&signal).expect("postcard serialize");
    let bin: &[u8] = &[];

    unsafe {
        rinf_send_dart_signal_signal(msg.as_ptr(), msg.len(), bin.as_ptr(), bin.len());
    }
}

/// Dummy signal type for empty handlers.
#[derive(Clone, Debug)]
pub struct EmptySignal;

impl DartSignal for EmptySignal {
    fn get_dart_signal_receiver() -> rinf::SignalReceiver<rinf::DartSignalPack<Self>> {
        // This should never be called for empty handlers
        unreachable!("Empty handlers should not receive signals")
    }
}

impl serde::Serialize for EmptySignal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_unit()
    }
}

impl<'de> serde::Deserialize<'de> for EmptySignal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_unit(serde::de::IgnoredAny)?;
        Ok(EmptySignal)
    }
}

/// Empty handler for testing - a noop.
pub async fn empty() {}

/// Signal handler.
pub async fn signal(_signal: Signal) {}

/// The empty handler implementation -- useful for testing.
impl<F, Fut, R, S> Handler<(), S> for F
where
    F: FnOnce() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    S: Clone + Send + Sync + 'static,
    R: IntoResponse + Send + 'static,
{
    type Future = BoxFuture<'static, ()>;
    type Signal = EmptySignal;

    fn call(self, _signal: Self::Signal, _state: S) -> Self::Future {
        Box::pin(async move {
            let result = self().await.into_response();
            result.send_signal_to_dart();
        })
    }
}

/// A middleware layer for tracking execution order with custom identifiers.
/// Useful for testing that middleware executes in the expected order.
#[derive(Clone)]
pub struct TrackingLayer<const N: usize>;

impl<const N: usize, S> Layer<S> for TrackingLayer<N> {
    type Service = TrackingService<N, S>;

    fn layer(&self, inner: S) -> Self::Service {
        TrackingService { inner }
    }
}

/// The service created by OrderTrackingLayer.
#[derive(Clone)]
pub struct TrackingService<const N: usize, S> {
    inner: S,
}

// Generic implementation for any DartSignal type with a message field
impl<const N: usize, S, T> Service<T> for TrackingService<N, S>
where
    S: Service<T, Response = (), Error = std::convert::Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    T: DartSignal + Send + 'static,
    // This is the key constraint - T must have a message field that can be modified
    T: MessageModifiable,
{
    type Error = std::convert::Infallible;
    type Future = BoxFuture<'static, Result<(), std::convert::Infallible>>;
    type Response = ();

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: T) -> Self::Future {
        req.modify_message(&format!("{N} "));

        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

/// Trait for signal types that can have their message modified by middleware
pub trait MessageModifiable {
    /// Modify the message content by appending a suffix
    fn modify_message(&mut self, suffix: &str);
}

impl MessageModifiable for Signal {
    fn modify_message(&mut self, suffix: &str) {
        self.message.push_str(suffix);
    }
}
