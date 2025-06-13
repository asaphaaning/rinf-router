# rinf-router

`rinf-router` is a tiny, ergonomic routing layer that glues Flutter’s
[RINF] signals to asynchronous Rust handlers.  
It takes care of the boring plumbing so you can focus on writing **clean,
testable** application logic.

[RINF]: https://pub.dev/packages/rinf

---

## Features

* Familiar, Axum-style API
* Zero-boilerplate extraction of data and shared state
* Fully async – powered by [`tokio`] and `async`/`await`
* Runs anywhere [RINF] runs (desktop, mobile, web)

### Upcoming features

* `tower` compatible
* Graceful shutdown support

[`tokio`]: https://tokio.rs/

---

## Quick-start

Add the crate to your existing [RINF] project.

```bash
cargo add rinf-router
```

A minimal example (run with `cargo run`):

```rust no_run
use {
    rinf_router::{Router, State},
    rinf::DartSignal,
    serde::Deserialize,
    std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    },
};

/// Shared state for all handlers
#[derive(Clone)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

/// A signal coming from Dart
#[derive(Deserialize, DartSignal)]
struct Increment;

async fn incr(State(state): State<AppState>, _msg: Increment) {
    // Atomically increase the counter and print the new value
    let new = state.counter.fetch_add(1, Ordering::Relaxed) + 1;
    println!("Counter is now: {new}");
}

#[tokio::main]
async fn main() {
    let state = AppState {
        counter: Arc::new(AtomicUsize::new(0)),
    };

    Router::new()
        .route(incr)
        .with_state(state) // 👈 inject shared state
        .run()
        .await;
}
```

That’s it – incoming `Increment` signals are automatically deserialized, and the current `AppState` is dropped right
into your handler!

---

## Common pitfall: mismatched states

A router carries exactly **one** state type.  
Trying to register handlers that need *different* states on the same
router without swapping the state fails to compile:

```rust compile_fail
use rinf_router::{Router, State};

#[derive(Clone)]
struct Foo;
#[derive(Clone)]
struct Bar;

async fn foo(State(_): State<Foo>) { unimplemented!() }
async fn bar(State(_): State<Bar>) { unimplemented!() }

fn main() {
    Router::new()
        .route(foo) // Router<Foo>
        .route(bar) // ❌ Router<Foo> expected, found handler that needs Bar
        .run();     //        ^^^  mismatched state type
}
```

Fix it by either

```rust,ignore
Router::new()
    .route(foo)
    .with_state(state)
    .route(bar)
    .with_state(other_state)
    .run()
    .await;
```

or by ensuring both handlers share the same state type.

## Learn more

Run `cargo doc --open` for the full API reference, including:

* Custom extractors
* Error handling

Enjoy – and feel free to open an issue or PR if you spot anything that
could be improved!

---

## Appendix: How Sending Data to Dart Works

### 1. Handler Return Values

Your handlers can **return values** that automatically get sent to Dart:

```rust
use rinf::{RustSignal, DartSignal};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, RustSignal)]
struct Response {
    message: String,
    count: u32,
}

// Handler that receives data from Dart and sends response back
async fn my_handler(input: SomeInput) -> (Response,) {
    let response = Response {
        message: "Hello from Rust!".to_string(),
        count: 42,
    };
    (response,) // This gets automatically sent to Dart!
}
```

### 2. The Trait `IntoResponse`

The crate uses the `IntoResponse` trait to convert handler return values into signals:

- **`(T,)`** - Single tuple sends the value to Dart
- **`()`** - Unit type sends nothing (wrapped in `DontSend`)
- **`Result<T, E>`** - Sends either success or error to Dart
- **`Option<T>`** - Sends value if `Some`, nothing if `None`
- **`DontSend<T>`** - Explicitly prevents sending to Dart

### 3. Automatic Process

The process works as follows:

1. Handler returns any type implementing `IntoResponse`
2. `rinf-router` calls `IntoResponse::into_response()`
3. Framework automatically calls `RustSignal::send_signal_to_dart()`

### 4. Examples of Return Types

```rust
// Send a single response
async fn handler1() -> (MyResponse,) { /* ... */ }

// Send nothing back
async fn handler2() -> () { /* ... */ }

// Send success or error
async fn handler3() -> Result<Success, Error> { /* ... */ }

// Conditionally send
async fn handler4() -> Option<Response> { /* ... */ }

// Explicitly don't send (for internal errors)
async fn handler5() -> Result<(), DontSend<InternalError>> { /* ... */ }
```

### Key Points

- **Automatic**: Return values are automatically sent to Dart
- **Type-safe**: Your response types must implement `RustSignal`
- **Flexible**: Support for `Result`, `Option`, and custom types
- **Opt-out**: Use `DontSend<T>` when you don't want to send anything
