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
//! ```rust no_run
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

use {
    crate::{BoxedHandlerFuture, handler::Handler, logging::log},
    std::marker::PhantomData,
    tokio::task::JoinSet,
};

/// Type-erased [`Handler`] implementation can be wrapped in a
/// [`Box<dyn ErasedBoxedHandler<S>>`], stored in a collection and executed
/// later without the caller knowing anything about the original generic
/// parameters.
///
/// * `S` – shared application state that is passed by value to every handler
///   invocation.
trait ErasedBoxedHandler<S>: Send + Sync + 'static {
    /// Consumes `self` and invokes the handler.
    fn handle(self: Box<Self>, state: S) -> BoxedHandlerFuture;
}

/// Helper that pairs a handler value with the function that can turn
/// that value into a boxed future.
///
/// This struct is never exposed publicly; it only exists to fulfil
/// [`ErasedBoxedHandler`] and therefore enable dynamic dispatch.
struct MakeBoxedHandler<H, S> {
    handler: H,
    into_future: fn(H, S) -> BoxedHandlerFuture,
}

impl<H, S> ErasedBoxedHandler<S> for MakeBoxedHandler<H, S>
where
    H: Send + Sync + 'static,
    S: 'static,
{
    fn handle(self: Box<Self>, state: S) -> BoxedHandlerFuture {
        (self.into_future)(self.handler, state)
    }
}

/// Internal enum used by [`Router`] to keep a list of *things that can be
/// turned into a running task*.
///
/// It can either be
/// * a concrete handler that still needs the application state
///   [`MakeBoxedHandler`], or
/// * an already-prepared task [`BoxedIntoRoute::Route`].
enum BoxedIntoRoute<S> {
    /// Temporary “route” that still needs `state`.
    MakeBoxedHandler(Box<dyn ErasedBoxedHandler<S>>),
    /// Route, primed and ready to serve requests.
    Route(Box<dyn Routable>),
}

impl<S> BoxedIntoRoute<S> {
    /// Type-erase the handlers signal, store only the state at the type-level.
    fn make<H, T>(h: H) -> Self
    where
        H: Handler<T, S> + Send + Sync + 'static,
        S: Send + Sync + 'static,
    {
        Self::MakeBoxedHandler(Box::new(MakeBoxedHandler {
            handler: h,
            into_future: |handler, state| Box::pin(handler.handle(state)),
        }))
    }

    /// Inject concrete application state, producing a spawn-ready route.
    fn into_route<S2>(self, state: S) -> BoxedIntoRoute<S2>
    where
        S: Clone + Send + Sync + 'static,
    {
        match self {
            Self::MakeBoxedHandler(boxed_handler) => {
                BoxedIntoRoute::Route(Box::new(move || boxed_handler.handle(state.clone())))
            },

            BoxedIntoRoute::Route(route) => BoxedIntoRoute::Route(route),
        }
    }
}

/// Describes something that turns a closure or function pointer into something
/// that can be *spawned* into a Tokio [`JoinSet`].
///
/// Users never implement this manually; it is blanket-implemented for
/// any `FnOnce() -> BoxedHandlerFuture`.
trait Routable: Send + Sync + 'static {
    /// Take ownership and spawn the underlying future into `set`.
    fn spawn_into(self: Box<Self>, set: &mut JoinSet<()>);
}

impl<F> Routable for F
where
    F: FnOnce() -> BoxedHandlerFuture + Send + Sync + 'static,
{
    fn spawn_into(self: Box<Self>, set: &mut JoinSet<()>) {
        set.spawn(async move { self().await });
    }
}

impl BoxedIntoRoute<()> {
    /// Used when calling [`Router::run`], starting all the handlers.
    fn spawn_into(self, set: &mut JoinSet<()>) {
        match self {
            Self::MakeBoxedHandler(boxed_handler) => {
                set.spawn(boxed_handler.handle(()));
            },
            BoxedIntoRoute::Route(route) => route.spawn_into(set),
        };
    }
}

/// ### Router
///
/// The `Router` type wires up **RINF** signals to asynchronous Rust
/// handlers.
///
/// There's only a single generic parameter, `S`, that represents **the single
/// shared state** carried through the whole router tree.
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
    pub const fn new() -> Self {
        Self {
            routes: Routes::new(),
            state: PhantomData,
        }
    }

    /// Adds a signal handler to the [`Router`]
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub fn route<T, H>(mut self, h: H) -> Self
    where
        H: Handler<T, S> + Send + Sync + 'static,
    {
        self.routes.route(BoxedIntoRoute::make::<H, T>(h));
        self
    }

    /// Finish registration – the only method that still knows about `S`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub fn with_state<S2>(self, state: S) -> Router<S2> {
        Router {
            routes: self.routes.with_state(state),
            state: PhantomData,
        }
    }
}

impl Router {
    /// Consume `self`, spawn every handler into a single `JoinSet`,
    /// and return the running router (`Router<()>`).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    pub async fn run(self) {
        let mut set = JoinSet::new();

        log!(info, "Starting router");

        self.routes.run(&mut set);

        // Drive the set forever.  If any task finishes, we just keep waiting;
        while let Some(res) = set.join_next().await {
            #[allow(clippy::redundant_pattern_matching)]
            if let Err(_) = res {
                // Log errors
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

    /// Inject state required by handlers that define said state through their
    /// signature and have been registered through [`Self::route`].
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "debug"))]
    fn with_state<S2>(self, state: S) -> Routes<S2> {
        let vec = self
            .0
            .into_iter()
            .map(move |route| route.into_route(state.clone()))
            .collect();

        Routes(vec)
    }
}

impl Routes {
    /// Spawn every route contained in `self` into the provided [`JoinSet`].
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all, level = "info"))]
    fn run(self, set: &mut JoinSet<()>) {
        for route in self.0.into_iter() {
            route.spawn_into(set);
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::State,
        futures::{FutureExt, poll},
        rinf::DartSignal,
        serde::{Deserialize, Serialize},
        serial_test::serial,
    };

    #[tokio::test]
    #[serial]
    async fn router_without_calling_run_does_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(1);

        let _router = Router::new().route(handler_with_state).with_state::<()>(tx);

        send_test_signal(TestSignal::new("Hello from Dart!"));

        assert!(poll!(rx.recv().boxed()).is_pending());
    }

    #[tokio::test]
    #[serial]
    async fn router_with_state() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(1);
        tokio::spawn(Router::new().route(handler_with_state).with_state(tx).run());

        send_test_signal(TestSignal::new("Hello from Dart!"));

        assert_eq!(rx.recv().await.unwrap(), 1);
    }

    #[tokio::test]
    #[serial]
    async fn router_with_multiple_handlers() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(1);

        tokio::spawn(
            Router::new()
                .route(handler_with_state)
                .with_state(tx)
                .route(handler_with_a_different_state)
                .with_state("delicious".to_string())
                .run(),
        );

        send_test_signal(TestSignal::new("Hello from Dart!"));

        assert_eq!(rx.recv().await.unwrap(), 1);
    }

    async fn handler_with_state(
        State(state): State<tokio::sync::mpsc::Sender<u8>>,
        signal: TestSignal,
    ) {
        dbg!("Handler called", signal);
        state.send(1).await.unwrap();
    }

    #[derive(Debug, Serialize, Deserialize, DartSignal)]
    struct TestSignal {
        message: String,
    }

    impl TestSignal {
        fn new(message: impl AsRef<str>) -> Self {
            Self {
                message: message.as_ref().to_string(),
            }
        }
    }

    async fn handler_with_a_different_state(State(state): State<String>, signal: TestSignal) {
        dbg!("Another handler called", signal);
        dbg!("State is", state);
    }

    fn send_test_signal<T: Serialize>(signal: T) {
        let message_bytes = rinf::serialize(&signal).expect("postcard serialize");
        let binary: &[u8] = &[];

        unsafe {
            rinf_send_dart_signal_test_signal(
                message_bytes.as_ptr(),
                message_bytes.len(),
                binary.as_ptr(),
                binary.len(),
            );
        }
    }
}
