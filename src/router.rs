//! ## Router
//!
//! Wires up signals to handlers.
//! Supplies state in the following manner (see README.md) for more information:
//!
//! ```text
//! Router<_>
//   ├─route(...StateA...)            -> Router<StateA>
//   └─with_state(StateA value)       -> Router<_>   // StateA sealed in
//   ├─route(...StateB...)            -> Router<StateB>
//   └─with_state(StateB value)       -> Router<_>   // StateB sealed in
//   ...
//! ```
//! 
//! The snippet below shows the smallest possible router.
//! It registers a single “do-nothing” handler and then starts the router.
//! ```rust,ignore
//! use rinf_router::Router;
//!
//! // A handler is just an async function.  In this toy example it receives
//! // no extracted arguments and does nothing. This is a no-op.
//! async fn empty_handler() {}
//!
//! #[tokio::main]
//! async fn main() {
//!     Router::new()             // Router<_>
//!         .route(empty_handler) // Router<()> – handler needs no state
//!         .run()                // start all registered handlers
//!         .await;
//! }
//! ```

use std::{convert::Infallible, marker::PhantomData};

use crate::{handler::Handler, logging::log, service::Route, BoxCloneService};
use tower::Layer;

/// Type-erased [`Handler`] implementation can be wrapped in a
/// [`Box<dyn ErasedBoxedHandler<S>>`], stored in a collection and executed
/// later without the caller knowing anything about the original generic
/// parameters.
///
/// * `S` – shared application state that is passed by value to every handler
///   invocation.
trait ErasedBoxedHandler<S>: Send + Sync + 'static {
    /// Consumes `self` and creates a Route service.
    fn into_route(self: Box<Self>, state: S) -> Route;
}

/// Helper that pairs a handler value with the function that can turn
/// that value into a Route service.
///
/// This struct is never exposed publicly; it only exists to fulfil
/// [`ErasedBoxedHandler`] and therefore enable dynamic dispatch.
struct MakeBoxedHandler<H, T, S> {
    handler: H,
    _phantom: PhantomData<fn() -> (T, S)>,
}

impl<H, T, S> ErasedBoxedHandler<S> for MakeBoxedHandler<H, T, S>
where
    H: Handler<T, S> + Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    fn into_route(self: Box<Self>, state: S) -> Route {
        let handler_service = self.handler.with_state(state);
        let signal_service = crate::service::SignalService::new(handler_service);

        Route::new(signal_service)
    }
}

struct LayeredBoxedHandler<S, L> {
    inner: Box<dyn ErasedBoxedHandler<S>>,
    layer: L,
}

impl<S, L> ErasedBoxedHandler<S> for LayeredBoxedHandler<S, L>
where
    S: Clone + Send + Sync + 'static,
    L: Layer<BoxCloneService> + Clone + Send + Sync + 'static,
    L::Service: tower::Service<(), Response = (), Error = Infallible>
        + Clone
        + Send
        + 'static,
    <L::Service as tower::Service<()>>::Future: Send + 'static,
{
    fn into_route(self: Box<Self>, state: S) -> Route {
        let route = self.inner.into_route(state);
        route.layer(self.layer)
    }
}

/// Internal enum used by [`Router`] to keep a list of *things that can be
/// turned into running services*.
///
/// It can either be
/// * a concrete handler that still needs the application state
///   [`MakeBoxedHandler`], or
/// * an already-prepared service [`BoxedIntoRoute::Route`].
enum BoxedIntoRoute<S> {
    /// Temporary "route" that still needs `state`.
    MakeBoxedHandler(Box<dyn ErasedBoxedHandler<S>>),
    /// Route, primed and ready to serve requests.
    Route(Route),
}

impl<S> BoxedIntoRoute<S> {
    /// Type-erase the handlers signal, store only the state at the type-level.
    fn make<H, T>(h: H) -> Self
    where
        H: Handler<T, S> + Send + Sync + 'static,
        H::Future: Send + 'static,
        S: Clone + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        Self::MakeBoxedHandler(Box::new(MakeBoxedHandler {
            handler: h,
            _phantom: PhantomData,
        }))
    }

    /// Inject concrete application state, producing a spawn-ready route.
    fn into_route(self, state: S) -> Route
    where
        S: Clone + Send + Sync + 'static,
    {
        match self {
            Self::MakeBoxedHandler(boxed_handler) => boxed_handler.into_route(state),
            BoxedIntoRoute::Route(route) => route,
        }
    }
}

/// ### Router
///
/// The `Router` type wires up **RINF** signals to asynchronous Rust
/// handlers.
///
/// The `Router` has a single generic parameter:
/// * `S` – the **single shared state** carried through the router tree.
///
/// ```txt
/// ╔═══════════════════╗                     ╔════════════════╗
/// ║                   ║░░                   ║                ║░░
/// ║  FLUTTER / DART   ║░░ ──── signal ────▶ ║  RINF-ROUTER   ║░░
/// ║                   ║░░                   ║     (RUST)     ║░░
/// ╚═══════════════════╝░░                   ╚══════╦═════════╝░░
///                                                 ║ extract
///                                                 ▼
///                       ╔══════════════════════════════════════════╗
///                       ║                                          ║░░
///                       ║ async fn incr(State<S>, Increment)       ║░░
///                       ║                                          ║░░
///                       ╚══════════════════════════════════════════╝░░
/// ```
///
/// Attempting to register handlers that require *different* state types on
/// the same router triggers a compile-time error.
pub struct Router<S = ()> {
    routes: Routes<S>,
    state: PhantomData<S>,
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a new [`Router`]
    pub fn new() -> Self {
        Self {
            routes: Routes::new(),
            state: PhantomData,
        }
    }
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Adds a signal handler to the [`Router`]
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub fn route<T: Send + Sync + 'static, H>(mut self, h: H) -> Self
    where
        H: Handler<T, S> + Send + Sync + 'static,
        H::Future: Send + 'static,
        H::Signal: Sync,
    {
        self.routes.route(BoxedIntoRoute::make::<H, T>(h));
        self
    }

    /// Apply a Tower layer to all routes in this router.
    ///
    /// This works like Axum's `Router::layer`: the layer wraps every existing
    /// route service registered on this router. Multiple layers compose in
    /// outside-in order.
    ///
    /// `Router::layer` only applies to routes that already exist. Call
    /// `layer` after the `route(...)` calls you want to wrap. It doesn't
    /// matter whether `with_state(...)` is called before or after; routes
    /// added afterwards will not be wrapped.
    ///
    /// ```rust,no_run
    /// use rinf_router::Router;
    /// use rinf::DartSignal;
    /// use serde::Deserialize;
    /// use tower::Layer;
    ///
    /// #[derive(Clone)]
    /// struct GlobalLayer;
    ///
    /// impl<S> Layer<S> for GlobalLayer {
    ///     type Service = S;
    ///
    ///     fn layer(&self, inner: S) -> Self::Service {
    ///         inner
    ///     }
    /// }
    ///
    /// #[derive(Deserialize, DartSignal)]
    /// struct Ping;
    ///
    /// async fn handler(_signal: Ping) {}
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     Router::new()
    ///         .route(handler)
    ///         .layer(GlobalLayer) // applied to existing routes above
    ///         .run()
    ///         .await;
    /// }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<BoxCloneService> + Clone + Send + Sync + 'static,
        L::Service: tower::Service<(), Response = (), Error = Infallible>
            + Clone
            + Send
            + 'static,
        <L::Service as tower::Service<()>>::Future: Send + 'static,
    {
        let routes = self
            .routes
            .0
            .into_iter()
            .map(|route| match route {
                BoxedIntoRoute::MakeBoxedHandler(boxed_handler) => {
                    BoxedIntoRoute::MakeBoxedHandler(Box::new(LayeredBoxedHandler {
                        inner: boxed_handler,
                        layer: layer.clone(),
                    }))
                },
                BoxedIntoRoute::Route(route) => {
                    BoxedIntoRoute::Route(route.layer(layer.clone()))
                },
            })
            .collect();

        Router {
            routes: Routes(routes),
            state: PhantomData,
        }
    }

    /// Finish registration by providing application state to all routes that
    /// need state. Routes that have already been converted to `Route`
    /// objects are left unchanged.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub fn with_state<S2>(self, state: S) -> Router<S2>
    where
        S2: Clone + Send + Sync + 'static,
    {
        let routes = self
            .routes
            .0
            .into_iter()
            .map(move |route| BoxedIntoRoute::Route(route.into_route(state.clone())))
            .collect();

        Router {
            routes: Routes(routes),
            state: PhantomData,
        }
    }
}

impl Router {
    /// Consume `self`, spawn every handler into a single `JoinSet`,
    /// and return the running router (`Router<()>`).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub async fn run(self) {
        log!(info, "Starting router");

        #[cfg(any(feature = "tokio-rt", feature = "tokio-rt-multi-thread"))]
        self.run_with_tokio().await;

        #[cfg(not(any(feature = "tokio-rt", feature = "tokio-rt-multi-thread")))]
        self.run_with_futures().await;
    }

    #[cfg(any(feature = "tokio-rt", feature = "tokio-rt-multi-thread"))]
    async fn run_with_tokio(self) {
        let mut set = tokio::task::JoinSet::new();

        for route in self.routes.0.into_iter() {
            let route_service = match route {
                BoxedIntoRoute::MakeBoxedHandler(boxed_handler) => boxed_handler.into_route(()),
                BoxedIntoRoute::Route(route) => route,
            };

            set.spawn(async move {
                use tower::ServiceExt;
                let _ = route_service.oneshot(()).await;
            });
        }

        // Race between all handlers completing and dart shutdown
        tokio::select! {
            _ = set.join_all() => {
                log!(info, "All handlers completed");
            }
            _ = rinf::dart_shutdown() => {
                log!(info, "Dart runtime shutting down");
            }
        }
    }

    #[cfg(not(any(feature = "tokio-rt", feature = "tokio-rt-multi-thread")))]
    async fn run_with_futures(self) {
        use futures::{FutureExt, StreamExt};

        let route_futures = self
            .routes
            .0
            .into_iter()
            .map(|route| {
                let route_service = match route {
                    BoxedIntoRoute::MakeBoxedHandler(boxed_handler) => boxed_handler.into_route(()),
                    BoxedIntoRoute::Route(route) => route,
                };

                async move {
                    use tower::ServiceExt;
                    let _ = route_service.oneshot(()).await;
                }
            })
            .collect::<futures::stream::FuturesUnordered<_>>();

        let all_handlers_done = async {
            let mut routes = route_futures;
            while routes.next().await.is_some() {}
        };

        futures::select! {
            _ = all_handlers_done.fuse() => {
                log!(info, "All handlers completed");
            }
            _ = rinf::dart_shutdown().fuse() => {
                log!(info, "Dart runtime shutting down");
            }
        }
    }
}

struct Routes<S = ()>(Vec<BoxedIntoRoute<S>>);

impl<S> Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Construct an empty set of routes.
    const fn new() -> Self {
        Self(Vec::new())
    }

    /// Append a new handler to the routing table.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "debug"))]
    fn route(&mut self, handler: BoxedIntoRoute<S>) -> &mut Self {
        self.0.push(handler);
        self
    }
}

#[cfg(test)]
#[cfg(feature = "test-helpers")]
mod tests {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    use futures::{future::BoxFuture, FutureExt, poll};
    use serial_test::serial;
    use tower::{Layer, Service};

    use super::*;
    use crate::{
        State,
        test_helpers::{Signal, TrackingLayer, empty, send_signal, signal},
    };

    async fn stateful_handler(State(state): State<tokio::sync::mpsc::Sender<u8>>, _signal: Signal) {
        state.send(1).await.unwrap();
    }

    #[derive(Clone)]
    struct RouteTrackingLayer {
        started: Arc<AtomicBool>,
    }

    impl<S> Layer<S> for RouteTrackingLayer {
        type Service = RouteTrackingService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            RouteTrackingService {
                inner,
                started: self.started.clone(),
            }
        }
    }

    #[derive(Clone)]
    struct RouteTrackingService<S> {
        inner: S,
        started: Arc<AtomicBool>,
    }

    impl<S> Service<()> for RouteTrackingService<S>
    where
        S: Service<(), Response = (), Error = Infallible> + Clone + Send + 'static,
        S::Future: Send + 'static,
    {
        type Error = Infallible;
        type Future = BoxFuture<'static, Result<(), Infallible>>;
        type Response = ();

        fn poll_ready(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, req: ()) -> Self::Future {
            self.started.store(true, Ordering::SeqCst);
            let mut inner = self.inner.clone();
            Box::pin(async move { inner.call(req).await })
        }
    }

    #[tokio::test]
    #[serial]
    async fn router_without_run_does_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(1);
        let _router = Router::new().route(stateful_handler).with_state::<()>(tx);
        send_signal(Signal::new("test"));
        assert!(poll!(rx.recv().boxed()).is_pending());
    }

    #[tokio::test]
    #[serial]
    async fn router_with_run_works() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(1);
        tokio::spawn(Router::new().route(stateful_handler).with_state(tx).run());
        send_signal(Signal::new("test"));
        assert_eq!(rx.recv().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn router_with_multiple_handlers() {
        // This test verifies that you can have multiple handlers with different state
        // types in the same router. We only test compilation/structure, not
        // signal processing to avoid race conditions between handlers listening
        // for the same signal type.
        let (tx, _rx) = tokio::sync::mpsc::channel::<u8>(1);
        let _router: Router<()> = Router::new()
            .route(stateful_handler) // Handles Signal type with Sender<u8> state
            .with_state(tx)
            .route(signal) // Handles Signal type with String state
            .with_state("state".to_string());

        // Test passes if the router can be constructed with multiple state
        // types
    }

    #[tokio::test]
    async fn test_router_compilation() {
        // Test basic router compilation
        let _router1: Router<()> = Router::new().route(signal);

        // Test multiple routes compilation
        let _router2: Router<()> = Router::new().route(signal).route(empty);
    }

    #[tokio::test]
    #[serial]
    async fn handler_level_middleware_execution_order() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        let handler = move |signal: Signal| async move {
            assert_eq!(signal.message, "1 2 ");
            tx.send(()).await.unwrap();
        };

        let layered = handler
            .layer(TrackingLayer::<2>) // Inner layer (executes second)
            .layer(TrackingLayer::<1>); // Outer layer (executes first)

        tokio::spawn(Router::new().route(layered).run());

        // TODO: Find a better way to signal that the router is ready
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        send_signal(Signal::new(""));
        rx.recv().await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn router_level_middleware_runs_before_handler() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        let started = Arc::new(AtomicBool::new(false));
        let started_for_handler = Arc::clone(&started);
        let handler = move |_signal: Signal| async move {
            assert!(started_for_handler.load(Ordering::SeqCst));
            tx.send(()).await.unwrap();
        };

        let layered = handler
            .layer(TrackingLayer::<2>) // Inner layer (executes second)
            .layer(TrackingLayer::<1>); // Outer layer (executes first)

        tokio::spawn(
            Router::new()
                .route(layered)
                .layer(RouteTrackingLayer { started }) // Applies to existing routes
                .run(),
        );

        // TODO: Find a better way to signal that the router is ready
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        send_signal(Signal::new(""));
        rx.recv().await.unwrap();
    }
}
