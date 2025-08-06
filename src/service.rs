//! ## Service Layer
//!
//! Tower service implementations for RINF router.

use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use rinf::DartSignal;
use tower::{Layer, Service};

use crate::{BoxCloneService, logging::log};

/// A [`Route`] is a Tower service that represents a single handler route.
#[derive(Clone)]
pub struct Route(BoxCloneService);

impl Route {
    /// Create a new [`Route`] from any Tower service.
    pub fn new<S>(service: S) -> Self
    where
        S: Service<(), Error = Infallible, Response = ()> + Clone + Send + 'static,
        S::Future: Send + 'static,
    {
        Self(BoxCloneService::new(service))
    }

    /// Apply a Tower layer to this route.
    pub fn layer<L>(self, layer: L) -> Route
    where
        L: Layer<BoxCloneService> + Clone + Send + 'static,
        L::Service: Service<(), Error = Infallible, Response = ()> + Clone + Send + 'static,
        <L::Service as Service<()>>::Future: Send + 'static,
    {
        Route::new(layer.layer(self.0))
    }
}

impl Service<()> for Route {
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<(), Infallible>> + Send>>;
    type Response = ();

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: ()) -> Self::Future {
        self.0.call(req)
    }
}

/// [`SignalService`] is the outermost service that manages the infinite loop
/// of receiving Dart signals and passing them to the inner service.
///
/// This service never returns - it runs forever listening for signals.
pub struct SignalService<S, T> {
    inner: S,
    _phantom: PhantomData<T>,
}

impl<S, T> SignalService<S, T>
where
    T: DartSignal + Send + 'static,
    S: Service<T> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    /// Create a new [`SignalService`] wrapping the given service.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            _phantom: PhantomData,
        }
    }
}

impl<S, T> Clone for SignalService<S, T>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<S, T> Service<()> for SignalService<S, T>
where
    T: DartSignal + Send + 'static,
    S: Service<T> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<(), Infallible>>;
    type Response = ();

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Always ready since we handle the infinite loop internally
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let inner = self.inner.clone();

        Box::pin(async move {
            log!(info, handler = %std::any::type_name::<T>(), "Starting handler service");

            while let Some(signal_pack) = T::get_dart_signal_receiver().recv().await {
                log!(debug, handler = %std::any::type_name::<T>(), "Received signal");

                match tower::ServiceExt::oneshot(inner.clone(), signal_pack.message).await {
                    Ok(_) => {},
                    Err(_) => {
                        log!(error, handler = %std::any::type_name::<T>(), "Handler service error");
                    },
                }
            }

            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg(feature = "test-helpers")]
mod tests {
    use std::sync::{Arc, Mutex};

    use tower::{Service, layer::layer_fn};

    use super::*;

    // Mock service that counts calls
    #[derive(Clone)]
    struct CountingService {
        counter: Arc<Mutex<usize>>,
    }

    impl CountingService {
        fn new() -> Self {
            Self {
                counter: Arc::new(Mutex::new(0)),
            }
        }

        fn call_count(&self) -> usize {
            *self.counter.lock().unwrap()
        }
    }

    impl Service<()> for CountingService {
        type Error = Infallible;
        type Future = BoxFuture<'static, Result<(), Infallible>>;
        type Response = ();

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: ()) -> Self::Future {
            let counter = Arc::clone(&self.counter);
            Box::pin(async move {
                *counter.lock().unwrap() += 1;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn test_route_creation_and_calling() {
        let service = CountingService::new();
        let route = Route::new(service.clone());

        let mut route_clone = route.clone();
        route_clone.call(()).await.unwrap();

        assert_eq!(service.call_count(), 1);
    }

    #[tokio::test]
    async fn test_route_with_middleware_layer() {
        let service = CountingService::new();
        let route = Route::new(service.clone());
        let layered_route = route.layer(layer_fn(std::convert::identity));
        let mut layered_route_clone = layered_route.clone();

        layered_route_clone.call(()).await.unwrap();

        // The underlying service should still be called
        assert_eq!(service.call_count(), 1);
    }

    #[tokio::test]
    async fn test_multiple_middleware_layers() {
        let service = CountingService::new();
        let route = Route::new(service.clone());
        let layered_route = route
            .layer(layer_fn(std::convert::identity))
            .layer(layer_fn(std::convert::identity));

        let mut layered_route_clone = layered_route.clone();
        layered_route_clone.call(()).await.unwrap();

        // Service should still be called once through all layers
        assert_eq!(service.call_count(), 1);
    }
}
