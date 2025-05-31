//! ## Extractor
//!
//! Re-usable pieces of code to extract data from a signal or the application
//! state.
//!
//! See [`FromRequest`] for more details.
//!
//! ## Examples
//!
//! ```rust no_run
//! use {
//!     rinf::DartSignal,
//!     rinf_router::{Router, State},
//!     serde::Deserialize,
//!     std::sync::Arc,
//! };
//!
//! // Define your application state
//! #[derive(Clone)]
//! struct AppState {
//!     some_number: Arc<i32>,
//! }
//!
//! // ..And your signals
//! #[derive(Deserialize, DartSignal)]
//! struct MyMessage(String);
//!
//! // Use `State` extractor in your handler
//! async fn handler(State(state): State<AppState>, message: MyMessage) {
//!     println!("Message: {}, State: {}", message.0, state.some_number);
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Register handler with router
//!     let app_state = AppState {
//!         some_number: Arc::new(42),
//!     };
//!
//!     Router::new()
//!         .route(handler)
//!         .with_state(app_state)
//!         .run()
//!         .await;
//! }
//! ```
//!
//! Now your handler holds a state

use async_trait::async_trait;

/// An asynchronous trait for extracting data from [`T`] and the state: [`S`].
///
/// This trait allows implementing types to define how they should be
/// constructed from incoming RINF signals. It is used to provide a consistent
/// way to access request data within handlers.
#[async_trait]
pub trait FromRequest<T, S>: Sized {
    /// Extracts and constructs Self from the given signal and application
    /// state.
    ///
    /// # Arguments
    /// * `req` - Reference to the signal
    /// * `state` - Reference to the shared application state
    async fn from_request(req: &T, state: &S) -> Self;
}

/// An extractor type for accessing the shared application state within
/// handlers.
///
/// This is the main mechanism to inject application context state into your
/// handlers.
pub struct State<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<T, S> for State<S>
where
    S: Clone + Send + Sync + 'static,
{
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(state), level = "trace"))]
    async fn from_request(_: &T, state: &S) -> Self {
        Self(state.clone())
    }
}
