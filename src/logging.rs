//! ## Logging
//!
//! Tiny helpers to log only when logging is enabled by the consumer.
//!
//! Use like:
//!
//! ```rust,ignore
//! log!(debug, "Registered a new route"; "type" = %std::any::type_name::<S>());
//! ```

#[cfg(feature = "logging")]
macro_rules! log {
    (trace, $($arg:tt)+) => { tracing::trace!($($arg)+) };
    (debug, $($arg:tt)+) => { tracing::debug!($($arg)+) };
    (info , $($arg:tt)+) => { tracing::info! ($($arg)+) };
    (warn , $($arg:tt)+) => { tracing::warn! ($($arg)+) };
    (error, $($arg:tt)+) => { tracing::error!($($arg)+) };
}

#[cfg(not(feature = "logging"))]
macro_rules! log {
    ($($any:tt)*) => {}; // compiles to nothing
}

pub(crate) use log;
