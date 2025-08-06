#![doc = include_str!("../README.md")]

use std::convert::Infallible;

pub mod extractor;
pub mod handler;
pub mod into_response;
pub(crate) mod logging;
pub mod router;
pub mod service;

#[cfg(feature = "test-helpers")]
pub mod test_helpers;

/// Type alias for boxed, clonable services used in RINF router.
type BoxCloneService = tower::util::BoxCloneService<(), (), Infallible>;

pub use extractor::State;
pub use into_response::IntoResponse;
#[doc(hidden)]
pub use rinf;
pub use router::Router;
