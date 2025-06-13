//! Helper utilities for ergonomic signal handling

/// A macro that enables direct return of RustSignal types from handlers
///
/// This macro is used to implement IntoResponse for a specific RustSignal type,
/// allowing it to be returned directly from handlers without wrapping in tuples.
///
/// # Example
///
/// ```
/// use rinf::{RustSignal, DartSignal};
/// use serde::{Serialize, Deserialize};
/// use rinf_router::enable_direct_return;
///
/// #[derive(Serialize, Deserialize, RustSignal, DartSignal)]
/// struct TodoList {
///     items: Vec<String>,
/// }
///
/// // Enable direct return for TodoList
/// enable_direct_return!(TodoList);
///
/// // Now you can return TodoList directly
/// async fn handler() -> TodoList {
///     TodoList { items: vec![] }
/// }
/// ```
#[macro_export]
macro_rules! enable_direct_return {
    ($type:ty) => {
        impl $crate::into_response::IntoResponse for $type {
            type Response = $type;

            fn into_response(self) -> Self::Response {
                self
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use rinf::{DartSignal, RustSignal};
    use serde::{Deserialize, Serialize};
    use crate::into_response::IntoResponse;

    #[derive(Debug, Serialize, Deserialize, DartSignal, RustSignal)]
    struct DirectReturnTestSignal {
        message: String,
    }

    // Enable direct return for our test signal
    crate::enable_direct_return!(DirectReturnTestSignal);

    #[test]
    fn test_direct_return_macro() {
        let signal = DirectReturnTestSignal {
            message: "Test message".to_string(),
        };

        // Use IntoResponse directly
        let response = signal.into_response();

        // Verify the response is a RustSignal by calling a method on it
        response.send_signal_to_dart();
    }

    // This is just a compile-time test to ensure the handler works with direct return
    #[allow(dead_code)]
    async fn test_handler(signal: DirectReturnTestSignal) -> DirectReturnTestSignal {
        // Return the signal directly without wrapping in a tuple
        DirectReturnTestSignal {
            message: signal.message,
        }
    }
}
