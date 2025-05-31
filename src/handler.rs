//! ## Handler
//!
//! Any async function that take a number of "extractors", [`FromRequest`], and
//! ends with a RINF signal type, is a handler capable of being registered in
//! the router.
//!
//! It's important to note that the order of arguments matter -- **All but the
//! last arguments are meant for extractors while the last is meant for the Dart
//! signal**.

use {
    crate::{extractor::FromRequest, logging::log},
    rinf::DartSignal,
    std::future::Future,
};

macro_rules! impl_handler {
    (
        [$($arg:ident),*], $last:ident
    ) => {
    #[allow(non_snake_case, unused_variables)]
    impl<F, Fut $(,$arg)*, $last, S> Handler<($($arg,)* $last,), S> for F
    where
        F: FnOnce($($arg,)* $last) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = ()> + Send,
        S: Send + Sync + 'static,
        $(
          $arg: FromRequest<$last, S> + Send,
        )*
        $last: DartSignal + Send,
    {
        type Future = std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

        #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
        fn handle(self, state: S) -> Self::Future {
            Box::pin(async move {
                let state = &state;
                let handler = self;

                log!(info, handler = %std::any::type_name::<$last>(), "Starting handler");
                while let Some($last) = $last::get_dart_signal_receiver().recv().await {
                    log!(debug, handler = %std::any::type_name::<$last>(), "Received signal");

                    $(
                      let $arg = $arg::from_request(&$last.message, state).await;
                    )*

                    (handler.clone())($($arg,)* $last.message).await;
                }
            })
        }
    }
    };
}

/// ### Handler
///
/// The [`Handler`] trait is an **internal abstraction** that turns an ordinary
/// async function into a long-running task capable of receiving and processing
/// RINF signals.
///
/// You will very rarely implement this trait manually—implementations are
/// generated for you by the [`impl_handler!`] macro for functions that:
///
/// * Are `async`
/// * Take **zero or more _extractors_** (types that implement [`FromRequest`])
///   as their **leading** parameters
/// * End with the concrete RINF [`DartSignal`] message type
///
/// In the generic parameters:
///
/// * `T` is the **tuple** of arguments accepted by the user-defined function
///   (all extractors **plus** the signal message itself).   You should not rely
///   on its shape directly—its only purpose is to let the compiler distinguish
///   between different handler signatures.
/// * `S` is the application-wide **shared state** that can be injected into a
///   handler via the [`State`] extractor.
///
/// In practice you will meet `Handler` only indirectly when you call
/// `Router::route(handler)`—every async function that satisfies the rules above
/// already implements `Handler`.
pub trait Handler<T, S>: Clone + Send {
    /// The future returned by [`handle`].
    ///
    /// Handlers are **fire-and-forget** tasks: they never produce an
    /// application-level response, therefore the future’s output type is
    /// `()`.
    type Future: Future<Output = ()> + Send + 'static;

    /// Convert the handler into a future that **starts the event listener** for
    /// the associated RINF signal.
    ///
    /// The future returned by `handle` performs the following steps:
    ///
    /// 1. Obtains the global receiver for the associated signal via `<Signal as
    ///    DartSignal>::get_dart_signal_receiver()`.
    /// 2. Waits for incoming messages in an infinite loop.
    /// 3. For every message, builds each extractor by calling
    ///    [`FromRequest::from_request`] in the order they appear in the
    ///    function signature.
    /// 4. Invokes the user-defined async function with the freshly-constructed
    ///    arguments.
    ///
    /// You normally do **not** call this method yourself; it is invoked by the
    /// router when you register the handler.
    fn handle(self, state: S) -> Self::Future;
}

/// The empty handler -- useful for testing.
impl<F, Fut, S> Handler<(), S> for F
where
    F: FnOnce() -> Fut + Clone + Send + 'static,
    Fut: Future<Output = ()> + Send,
    S: Send + Sync + 'static,
{
    type Future = std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    fn handle(self, _: S) -> Self::Future {
        Box::pin(async move { self().await })
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    mod handler {
        use {super::*, crate::State, rinf::DartSignal, serde::Deserialize};

        #[derive(Debug, Deserialize, DartSignal)]
        struct Signal {
            _message: String,
        }

        async fn empty_handler() {}
        async fn normal_handler(_: Signal) {}
        async fn handler_with_state(State(_): State<String>, _: Signal) {}

        #[tokio::test]
        async fn handlers() {
            assert_handler::<_, (), _>(empty_handler);
            assert_handler::<_, (), _>(normal_handler);
            assert_handler(handler_with_state);
        }

        fn assert_handler<T, S, H: Handler<T, S>>(handler: H) -> H {
            handler
        }
    }
}
