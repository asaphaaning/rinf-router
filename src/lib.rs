
#![doc = include_str!("../README.md")]
#![allow(clippy::module_name_repetitions, clippy::new_without_default)]
#![warn(
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links,
    clippy::cargo,
    clippy::cast_lossless,
    clippy::checked_conversions,
    clippy::clone_on_ref_ptr,
    clippy::inefficient_to_string,
    clippy::large_enum_variant,
    clippy::large_stack_arrays,
    clippy::missing_panics_doc,
    clippy::panic_in_result_fn,
    clippy::rc_buffer,
    clippy::redundant_clone,
    clippy::redundant_closure_for_method_calls,
    clippy::unnecessary_wraps
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![deny(missing_docs, clippy::correctness, clippy::suspicious, clippy::perf)]
#![cfg_attr(not(test), deny(clippy::unwrap_in_result, clippy::expect_used))]
#![cfg_attr(not(test), forbid(unsafe_code))]

use std::{future::Future, pin::Pin};

pub mod extractor;
pub mod handler;
pub mod into_response;
pub(crate) mod logging;
pub mod router;

#[cfg(not(all(
    target_arch = "wasm32",
    target_vendor = "unknown",
    target_os = "unknown"
)))]
pub(crate) use tokio;

#[cfg(all(
    target_arch = "wasm32",
    target_vendor = "unknown",
    target_os = "unknown"
))]
pub(crate) use tokio_with_wasm::alias as tokio;

/// A boxed [`Future`] returned by any handler.
type BoxedHandlerFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

#[doc(hidden)]
pub use rinf;
pub use {extractor::State, into_response::IntoResponse, router::Router};
