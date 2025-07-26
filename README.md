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
* `tower` compatibility (Basic features, more to come)

### Upcoming features

* Graceful shutdown support

[`tokio`]: https://tokio.rs/

---

## Quick-start

Add the crate to your existing [RINF] project.

```bash
cargo add rinf-router
```

A minimal example (run with `cargo run`):

```rust,no_run
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

