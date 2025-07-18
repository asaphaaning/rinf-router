# Changelog

All notable changes to this project are documented in this file.

## [1.2.0]

### Added

- **Runtime features**
    - `rt-agnostic` (default) – pure `futures` backend.
    - `tokio-rt` / `tokio-rt-multi-thread` – Tokio back-ends using `JoinSet`.
- **Auto-backend selection** in `Router::run`:
    - Tokio feature → handlers spawn into `JoinSet`.
    - Otherwise → driven by one `FuturesUnordered`.
- `futures` and `tokio` set to `optional = true`.

### Changed

- Default build is now runtime-agnostic; Tokio users must enable a Tokio feature.
- Internal clean-up: dropped private `Routable`, simplified route storage.
- Bumped `rinf` to **8.6.0**.

### Breaking

- `Router<S>` is no longer `Sync` under the default feature.  
  Wrap in `Arc<Mutex<_>>` **or** compile with a Tokio feature to regain `Sync`.

### Upgrade snippet

```toml
# Generic backend (new default)
rinf-router = "1.2"

# Tokio single-thread
rinf-router = { version = "1.2", default-features = false, features = ["tokio-rt"] }

# Tokio multi-thread
rinf-router = { version = "1.2", default-features = false, features = ["tokio-rt-multi-thread"] }
```

## [1.1.0]

### Added

- **`into_response` module** introducing the `IntoResponse` trait for turning handler return values into `RustSignal`s.
- Helper types:
    - `DontSend<T>` for values that shouldn’t be forwarded to Dart.
    - Internal `Empty` and `Either<A, B>` signal wrappers.
- Blanket `IntoResponse` impls for `()`, `(T,)`, `Result<T, E>` and `Option<T>`.
- Automatic `send_signal_to_dart()` call after each handler finishes.

### Changed

- `Handler` now supports any return type that implements `IntoResponse`; its `Future` alias switched to
  `BoxFuture<'static, ()>`.
- Existing signal structs derive `RustSignal`.
- Minor wording tweaks in API docs.

---

## [1.1.0]

Initial commit