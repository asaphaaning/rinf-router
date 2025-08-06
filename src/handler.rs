//! ## Handler
//!
//! Any async function that take a number of "extractors", [`FromRequest`], and
//! ends with a RINF signal type, is a handler capable of being registered in
//! the router.
//!
//! It's important to note that the order of arguments matter -- **All but the
//! last arguments are meant for extractors while the last is meant for the Dart
//! signal**.

use std::{convert::Infallible, future::Future};

use futures::future::BoxFuture;
use rinf::{DartSignal, RustSignal};

use crate::{extractor::FromRequest, into_response::IntoResponse};

/// A thin Service wrapper that holds a Handler and its state.
pub struct HandlerService<H, T, S> {
    handler: H,
    state: S,
    _phantom: std::marker::PhantomData<fn() -> T>,
}

impl<H, T, S> Clone for HandlerService<H, T, S>
where
    H: Clone,
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            state: self.state.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<H, T, S> HandlerService<H, T, S>
where
    H: Clone,
    S: Clone,
{
    /// Create a new HandlerService with handler and state.
    pub fn new(handler: H, state: S) -> Self {
        Self {
            handler,
            state,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<H, T, S> tower::Service<H::Signal> for HandlerService<H, T, S>
where
    H: Handler<T, S>,
    H::Future: Send + 'static,
    S: Clone,
{
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<(), Infallible>>;
    type Response = ();

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, signal: H::Signal) -> Self::Future {
        let future = self.handler.clone().call(signal, self.state.clone());
        Box::pin(async move {
            future.await;
            Ok(())
        })
    }
}

macro_rules! impl_handler {
    (
        [$($arg:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case)]
        // Handler trait implementation for async functions
        impl<F, Fut, R $(,$arg)*, $last, S> Handler<($($arg,)* $last,), S> for F
        where
            F: FnOnce($($arg,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = R> + Send + 'static,
            S: Clone + Send + Sync + 'static,
            R: IntoResponse + Send + 'static,
            $(
              $arg: FromRequest<$last, S> + Send + 'static,
            )*
            $last: DartSignal + Send + Sync + 'static,
        {
            type Signal = $last;
            type Future = BoxFuture<'static, ()>;


            #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
            fn call(self, signal: $last, state: S) -> Self::Future {
                Box::pin(async move {
                    // Allow unused state when no extractors are present
                    #[allow(unused_variables)]
                    let _ = &state;

                    // Extract all the extractors
                    $(
                        let $arg = $arg::from_request(&signal, &state).await;
                    )*

                    // Call the async function
                    let result = self($($arg,)* signal).await.into_response();

                    // Send response back to Dart
                    result.send_signal_to_dart();
                })
            }
        }
    };
}

/// ### Handler
///
/// The [`Handler`] trait represents an async function that can process RINF
/// signals.
///
/// You will very rarely implement this trait manually—implementations are
/// generated for you by the [`impl_handler!`] macro for functions that:
///
/// * Are `async`
/// * Take **zero or more _extractors_** (types that implement [`FromRequest`])
///   as their **leading** parameters
/// * End with the concrete RINF [`DartSignal`] message type
///
/// The Handler trait focuses on calling the async function with proper
/// extractors. To turn a Handler into a Tower Service, use
/// [`HandlerWithoutStateExt::into_service`] on a [`Handler`].
pub trait Handler<T, S>: Clone + Send + Sized {
    /// The specific RINF signal type this handler processes.
    type Signal: DartSignal + Send + Sync + 'static;

    /// The future returned by calling this handler.
    type Future: Future<Output = ()> + Send + 'static;

    /// Call the handler with the given signal and state.
    fn call(self, signal: Self::Signal, state: S) -> Self::Future;

    /// Apply a Tower layer to this handler, creating a new layered handler.
    fn layer<L>(self, layer: L) -> Layered<L, Self, T, S>
    where
        L: tower::Layer<HandlerService<Self, T, S>> + Clone + Send + 'static,
        L::Service: tower::Service<Self::Signal, Response = (), Error = Infallible>
            + Clone
            + Send
            + 'static,
        S: Clone + Send + Sync + 'static,
    {
        Layered {
            layer,
            handler: self,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Convert this handler into a Tower Service with state.
    /// This creates a HandlerService wrapper around the handler.
    fn with_state(self, state: S) -> HandlerService<Self, T, S>
    where
        S: Clone + Send + Sync + 'static,
    {
        HandlerService::new(self, state)
    }
}

/// A handler wrapped with a Tower layer for middleware.
///
/// This type is returned by [`Handler::layer`] and implements [`Handler`]
/// itself, allowing you to compose multiple layers or pass it to
/// [`Router::route`].
pub struct Layered<L, H, T, S> {
    layer: L,
    handler: H,
    _phantom: std::marker::PhantomData<fn() -> (T, S)>,
}

impl<L, H, T, S> Clone for Layered<L, H, T, S>
where
    L: Clone,
    H: Clone,
{
    fn clone(&self) -> Self {
        Self {
            layer: self.layer.clone(),
            handler: self.handler.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<L, H, T, S> Handler<T, S> for Layered<L, H, T, S>
where
    H: Handler<T, S>,
    L: tower::Layer<HandlerService<H, T, S>> + Clone + Send + 'static,
    L::Service:
        tower::Service<H::Signal, Response = (), Error = Infallible> + Clone + Send + 'static,
    <L::Service as tower::Service<H::Signal>>::Future: Send + 'static,
    S: Clone + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    type Future = BoxFuture<'static, ()>;
    type Signal = H::Signal;

    fn call(self, signal: Self::Signal, state: S) -> Self::Future {
        let svc = self.handler.with_state(state);
        let svc = self.layer.layer(svc);

        Box::pin(async move {
            use tower::ServiceExt;
            let _ = svc.oneshot(signal).await;
        })
    }
}

/// Extension trait for handlers that don't require state.
pub trait HandlerWithoutStateExt<T>: Handler<T, ()> + Sized {
    /// Convert this handler into a service without requiring state.
    fn into_service(self) -> HandlerService<Self, T, ()> {
        Handler::with_state(self, ())
    }
}

impl<H, T> HandlerWithoutStateExt<T> for H where H: Handler<T, ()> {}

impl_handler!([], T1);
impl_handler!([T1], T2);
impl_handler!([T1, T2], T3);
impl_handler!([T1, T2, T3], T4);
impl_handler!([T1, T2, T3, T4], T5);
impl_handler!([T1, T2, T3, T4, T5], T6);
impl_handler!([T1, T2, T3, T4, T5, T6], T7);
impl_handler!([T1, T2, T3, T4, T5, T6, T7], T8);
impl_handler!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
impl_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
impl_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
impl_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
impl_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
impl_handler!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13],
    T14
);
impl_handler!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14],
    T15
);
impl_handler!(
    [
        T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
    ],
    T16
);

#[cfg(all(test, feature = "test-helpers"))]
mod tests {
    use std::sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use serial_test::serial;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        State,
        test_helpers::{EmptySignal, Signal, assert_handler},
    };

    // Compilation tests for various handler signatures
    #[tokio::test]
    #[allow(unused_must_use)]
    async fn handler_signatures_compile() {
        async fn empty_handler() {}
        async fn signal_handler(_: Signal) {}
        async fn stateful_handler(State(_): State<String>, _: Signal) {}
        async fn response_handler(State(_): State<String>, _: Signal) -> Option<()> {
            None
        }

        assert_handler::<_, (), _>(empty_handler);
        assert_handler::<_, (), _>(signal_handler);
        assert_handler(stateful_handler);
        assert_handler(response_handler);
    }

    #[tokio::test]
    async fn into_service_converts_stateless_handler() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let handler = move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        };

        let service = handler.into_service();
        service.oneshot(EmptySignal).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    #[serial]
    async fn handler_processes_signal() {
        let received = Arc::new(Mutex::new(String::new()));
        let received_clone = Arc::clone(&received);

        let handler = move |signal: Signal| {
            let received = Arc::clone(&received_clone);
            async move {
                *received.lock().unwrap() = signal.message;
            }
        };

        handler.call(Signal::new("test"), ()).await;
        assert_eq!(*received.lock().unwrap(), "test");
    }
}
