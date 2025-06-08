# Changelog

All notable changes to this project are documented in this file.

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