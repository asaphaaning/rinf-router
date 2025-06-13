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

## **Appendix: Return Types & Responses**
`rinf-router` provides a flexible system for sending data back to Dart from your handlers. Understanding how return types work will help you build more ergonomic and maintainable applications.
### **How It Works**
When your handler returns a value, `rinf-router` automatically:
1. **Converts** the return value using the `IntoResponse` trait
2. **Extracts** the underlying `RustSignal`
3. **Sends** it to Dart via `send_signal_to_dart()`

This happens transparently—you just return what you need, and the framework handles the rest!
### **Built-in Return Types**
#### **1. Unit Type `()` - Nothing to Send**
``` rust
async fn fire_and_forget_handler(cmd: DeleteCommand) {
    // Perform some action, but don't send anything back
    database.delete(cmd.id).await;
    // Returning () sends nothing to Dart
}
```
#### **2. Tuple `(T,)` - Send a Signal**
The classic RINF pattern for sending signals back to Dart:
``` rust
async fn classic_handler(req: GetUserRequest) -> (UserResponse,) {
    let user = database.get_user(req.id).await;
    (UserResponse { user },)  // ✅ Sent to Dart
}
```
#### **3. `Option<T>` - Conditional Responses**
Send a signal only when you have data:
``` rust
async fn maybe_respond(req: SearchRequest) -> Option<(SearchResults,)> {
    let results = search_engine.search(&req.query).await;

    if results.is_empty() {
        None  // ❌ Nothing sent to Dart
    } else {
        Some((SearchResults { results },))  // ✅ Sent to Dart
    }
}
```
#### **4. `Result<T, E>` - Error Handling**
Handle success and error cases elegantly:
``` rust
async fn fallible_handler(req: PaymentRequest) -> Result<(PaymentSuccess,), (PaymentError,)> {
    match payment_processor.charge(&req).await {
        Ok(transaction) => Ok((PaymentSuccess { transaction },)),     // ✅ Success sent to Dart
        Err(err) => Err((PaymentError { reason: err.to_string() },)), // ❌ Error sent to Dart
    }
}
```
#### **5. `DontSend<T>` - Keep Data in Rust**
Sometimes you need a return type for control flow, but don't want to send anything to Dart:
``` rust
use rinf_router::into_response::DontSend;

async fn internal_handler(cmd: LogCommand) -> DontSend<String> {
    let log_entry = format!("User action: {}", cmd.action);
    logger.write(&log_entry).await;

    DontSend(log_entry)  // ❌ Log stays in Rust, nothing sent to Dart
}
```
### **Direct Return**
For the cleanest API, use the macro to return signals directly without tuple wrapping: `enable_direct_return!`
``` rust
use rinf_router::enable_direct_return;

#[derive(Serialize, Deserialize, RustSignal, DartSignal)]
struct TodoList {
    items: Vec<TodoItem>,
    pending_count: u32,
}

// Enable direct return for this type
enable_direct_return!(TodoList);

async fn get_todos(State(state): State<AppState>, _: GetTodosCommand) -> TodoList {
    let todos = state.todos.lock().await;
    TodoList {
        items: todos.clone(),
        pending_count: todos.iter().filter(|t| !t.completed).count() as u32,
    }  // ✅ Clean return - automatically sent to Dart!
}
```
### **Response Flow Diagram**
``` 
Handler Return → IntoResponse → RustSignal → send_signal_to_dart() → Dart
     ↓              ↓              ↓              ↓
   MyData   →  (MyData,)    →   MyData    →    Flutter
   None     →   DontSend    →   DontSend   →     ❌ 
   Ok(data) →  Either::A    →    data     →    Flutter
   Err(e)   →  Either::B    →     e       →    Flutter
```
### **Custom Return Types**
You can implement for your own types: `IntoResponse`
``` rust
impl IntoResponse for MyCustomType {
    type Response = MySignal;  // Must implement RustSignal

    fn into_response(self) -> Self::Response {
        MySignal {
            data: self.process(),
        }
    }
}
```
