//! Utilities for converting handler return values into signals that can be
//! forwarded to Dart.
//!
//! The module defines the [`IntoResponse`] trait together with a set of blanket
//! implementations that cover the most common handler return types. A value
//! that implements [`IntoResponse`] is transparently turned into a concrete
//! [`RustSignal`] and its [`RustSignal::send_signal_to_dart`] method is invoked
//! once the handler completes.
//!
//! ## How it works
//! 1. A handler returns any type that implements [`IntoResponse`].
//! 2. `rinf-router` calls [`IntoResponse::into_response`] to obtain a value
//!    that implements [`RustSignal`].
//! 3. Finally, the framework calls [`RustSignal::send_signal_to_dart`] to
//!    forward the message to the Dart side.
//!
//! ## Extensibility
//! If your custom type already implements [`RustSignal`], implementing
//! [`IntoResponse`] is as simple as returning `self`. For composite types you
//! can follow the pattern used by the internal [`private::Either`] enum.
//!
//! ## Example
//! ```
//! use {
//!     rinf::RustSignal,
//!     serde::{Deserialize, Serialize},
//! };
//!
//! #[derive(Serialize, Deserialize, rinf::DartSignal, rinf::RustSignal)]
//! struct Ping(String);
//!
//! // A handler that echoes the incoming message back to Dart.
//! async fn echo(Ping(msg): Ping) -> (Ping,) {
//!     (Ping(msg),)
//! }
//! ```

use {
    rinf::RustSignal,
    serde::{Deserialize, Serialize},
};

/// Converts a handler’s return value into a concrete [`RustSignal`] that can be
/// delivered to Dart.
///
/// `rinf-router` provides implementations for `()`, [`DontSend<T>`], `(T,)`,
/// `Result<T,E>`, and `Option<T>`. You can also implement this trait for your
/// own types to give them first‑class support as handler return values.
pub trait IntoResponse {
    /// The signal type produced after the conversion. It **must** implement
    /// [`RustSignal`].
    type Response: RustSignal;

    /// Perform the conversion and return the signal that will be forwarded to
    /// Dart.
    fn into_response(self) -> Self::Response;
}

mod private {
    use super::{Deserialize, RustSignal, Serialize};

    #[derive(Serialize, Deserialize)]
    pub enum Either<A, B> {
        A(A),
        B(B),
    }

    impl<A, B> RustSignal for Either<A, B>
    where
        A: RustSignal,
        B: RustSignal,
    {
        fn send_signal_to_dart(&self) {
            match self {
                Either::A(a) => a.send_signal_to_dart(),
                Either::B(b) => b.send_signal_to_dart(),
            }
        }
    }
}

/// Wrapper for values that should remain on the Rust side.
///
/// Use this when you need a return type for ergonomic reasons but you do
/// **not** want the payload to be sent to Dart. The wrapper itself implements
/// [`RustSignal`] with a no‑op `send_signal_to_dart`.
///
/// ## Example
///
/// ```rust
/// ///
/// use {
///     rinf::RustSignal,
///     rinf_router::{handler::Handler, into_response::DontSend},
///     serde::{Deserialize, Serialize},
/// };
///
/// #[derive(Serialize, Deserialize, rinf::RustSignal)]
/// struct InternalError;
///
/// // The error is wrapped in `DontSend`, so it will *not* be forwarded to Dart.
/// async fn handler() -> Result<(), DontSend<InternalError>> {
///     // Business logic here…
///     Ok(())
/// }
///
/// #[cfg(feature = "test-helpers")]
/// rinf_router::test_helpers::assert_handler::<_, (), _>(handler);
/// ```
#[derive(Serialize)]
pub struct DontSend<T = ()>(pub T);

impl<T> RustSignal for DontSend<T>
where
    T: Serialize,
{
    fn send_signal_to_dart(&self) {}
}

impl<T> IntoResponse for DontSend<T>
where
    T: Serialize,
{
    type Response = Self;

    fn into_response(self) -> Self::Response {
        self
    }
}

impl IntoResponse for () {
    type Response = DontSend;

    fn into_response(self) -> Self::Response {
        DontSend(())
    }
}

impl<T, E> IntoResponse for Result<T, E>
where
    T: IntoResponse,
    E: IntoResponse,
{
    type Response = private::Either<T::Response, E::Response>;

    fn into_response(self) -> Self::Response {
        match self {
            Ok(ok) => private::Either::A(ok.into_response()),
            Err(err) => private::Either::B(err.into_response()),
        }
    }
}

impl<T> IntoResponse for Option<T>
where
    T: IntoResponse,
{
    type Response = private::Either<T::Response, DontSend>;

    fn into_response(self) -> Self::Response {
        match self {
            Some(val) => private::Either::A(val.into_response()),
            None => private::Either::B(DontSend(())),
        }
    }
}

impl<T> IntoResponse for (T,)
where
    T: RustSignal,
{
    type Response = T;

    fn into_response(self) -> Self::Response {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_into_response_compiles() {
        let resp = ().into_response();
        resp.send_signal_to_dart();
    }

    #[test]
    fn dont_send_round_trip() {
        let ok: Result<(), DontSend<&'static str>> = Ok(());
        let _ = ok.into_response();

        let err: Result<(), DontSend<&'static str>> = Err(DontSend("oops"));
        let _ = err.into_response();
    }

    #[test]
    fn option_into_response() {
        let some = Some(()).into_response();
        some.send_signal_to_dart();

        let none: Option<()> = None;
        let resp = none.into_response();
        resp.send_signal_to_dart();
    }
}
