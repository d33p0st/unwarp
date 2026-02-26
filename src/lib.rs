use std::{future::Future, net::SocketAddr};
use serde::{de::DeserializeOwned, Serialize};
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

/// Re-exported so the `unwarp!` macro can reference `$crate::warp` without
/// requiring downstream crates to add `warp` to their own `Cargo.toml`.
pub use warp;

/// A fully type-erased warp route whose output has been normalised to
/// [`warp::reply::Response`].  Produced by the `handle` methods on
/// [`RouteBuilder`], [`JsonRouteBuilder`] and [`QueryRouteBuilder`].
pub type _Filter = BoxedFilter<(warp::reply::Response, )>;

/// Supported HTTP methods for [`RouteBuilder`].
#[derive(Clone, Copy, Debug)]
enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

/// Box a method filter so all arms of the match have the same type.
fn method_filter(method: HttpMethod) -> BoxedFilter<()> {
    match method {
        HttpMethod::Get     => warp::get().boxed(),
        HttpMethod::Post    => warp::post().boxed(),
        HttpMethod::Put     => warp::put().boxed(),
        HttpMethod::Delete  => warp::delete().boxed(),
        HttpMethod::Patch   => warp::patch().boxed(),
        HttpMethod::Head    => warp::head().boxed(),
        HttpMethod::Options => warp::options().boxed(),
    }
}

// ── Path helper ──────────────────────────────────────────────────────────────

/// Parse a `"/"` separated path string into a boxed warp path filter.
///
/// Each segment is leaked to `&'static str`; this is fine because routes are
/// registered once at server start-up and live for the duration of the process.
fn path_filter(path: &str) -> BoxedFilter<()> {
    let segs: Vec<&'static str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) })
        .collect();

    if segs.is_empty() {
        return warp::path::end().boxed();
    }

    // Start with the first segment, then and-chain the rest.
    let mut f: BoxedFilter<()> = warp::path(segs[0]).boxed();
    for seg in &segs[1..] {
        f = f.and(warp::path(*seg)).boxed();
    }
    f.and(warp::path::end()).boxed()
}

// ── RouteBuilder ─────────────────────────────────────────────────────────────

/// Builder for a single warp route.
///
/// # Quick-start
/// ```rust
/// // GET /ping  →  plain-text response
/// let route = RouteBuilder::get("ping")
///     .handle(|| async { Ok::<_, Rejection>(warp::reply::html("pong")) });
///
/// // POST /echo  with a JSON body
/// let route = RouteBuilder::post("echo")
///     .json::<MyPayload>()
///     .handle(|p: MyPayload| async move { Ok::<_, Rejection>(warp::reply::json(&p)) });
///
/// // GET /search  with query params
/// let route = RouteBuilder::get("search")
///     .query::<SearchQuery>()
///     .handle(|q: SearchQuery| async move { Ok::<_, Rejection>(warp::reply::json(&q)) });
/// ```
pub struct RouteBuilder {
    method: HttpMethod,
    path: String,
}

impl RouteBuilder {
    /// Create a route with an explicit method.
    fn new(method: HttpMethod, path: impl Into<String>) -> Self {
        Self { method, path: path.into() }
    }

    pub fn get(path: impl Into<String>) -> Self { Self::new(HttpMethod::Get, path) }
    pub fn post(path: impl Into<String>) -> Self { Self::new(HttpMethod::Post, path) }
    pub fn put(path: impl Into<String>) -> Self { Self::new(HttpMethod::Put, path) }
    pub fn delete(path: impl Into<String>) -> Self { Self::new(HttpMethod::Delete, path) }
    pub fn patch(path: impl Into<String>) -> Self { Self::new(HttpMethod::Patch, path) }
    pub fn head(path: impl Into<String>) -> Self { Self::new(HttpMethod::Head, path) }
    pub fn options(path: impl Into<String>) -> Self { Self::new(HttpMethod::Options, path) }

    // ── body-type transitions ─────────────────────────────────────────────

    /// Expect a JSON body of type `T` (requires `T: DeserializeOwned`).
    pub fn json<T: DeserializeOwned + Send>(self) -> JsonRouteBuilder<T> {
        JsonRouteBuilder { base: self, _phantom: std::marker::PhantomData }
    }

    /// Expect URL query parameters deserialised into `T` (requires `T: DeserializeOwned`).
    pub fn query<T: DeserializeOwned + Send>(self) -> QueryRouteBuilder<T> {
        QueryRouteBuilder { base: self, _phantom: std::marker::PhantomData }
    }

    // ── terminal: attach a handler ────────────────────────────────────────

    /// Attach a handler that receives **no** extracted body or params.
    ///
    /// `handler` must be `Fn() -> impl Future<Output = Result<impl Reply, Rejection>>`.
    pub fn handle<F, Fut, Re>(self, handler: F) -> _Filter
    where
        F: Fn() -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<Re, Rejection>> + Send,
        Re: Reply + Send,
    {
        let m = method_filter(self.method);
        let p = path_filter(&self.path);
        m.and(p)
            .and_then(handler)
            .map(|r: Re| r.into_response())
            .boxed()
    }
}

// ── JsonRouteBuilder ─────────────────────────────────────────────────────────

/// A [`RouteBuilder`] that has been configured to deserialise a JSON body into `T`.
pub struct JsonRouteBuilder<T> {
    base: RouteBuilder,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned + Send + 'static> JsonRouteBuilder<T> {
    /// Attach a handler that receives the deserialised JSON body as `T`.
    ///
    /// `handler` must be `Fn(T) -> impl Future<Output = Result<impl Reply, Rejection>>`.
    pub fn handle<F, Fut, Re>(self, handler: F) -> _Filter
    where
        F: Fn(T) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<Re, Rejection>> + Send,
        Re: Reply + Send,
    {
        let m = method_filter(self.base.method);
        let p = path_filter(&self.base.path);
        m.and(p)
            .and(warp::body::json::<T>())
            .and_then(handler)
            .map(|r: Re| r.into_response())
            .boxed()
    }
}

// ── QueryRouteBuilder ────────────────────────────────────────────────────────

/// A [`RouteBuilder`] that has been configured to deserialise URL query parameters into `T`.
pub struct QueryRouteBuilder<T> {
    base: RouteBuilder,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned + Send + 'static> QueryRouteBuilder<T> {
    /// Attach a handler that receives the deserialised query parameters as `T`.
    ///
    /// `handler` must be `Fn(T) -> impl Future<Output = Result<impl Reply, Rejection>>`.
    pub fn handle<F, Fut, Re>(self, handler: F) -> _Filter
    where
        F: Fn(T) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<Re, Rejection>> + Send,
        Re: Reply + Send,
    {
        let m = method_filter(self.base.method);
        let p = path_filter(&self.base.path);
        m.and(p)
            .and(warp::filters::query::query::<T>())
            .and_then(handler)
            .map(|r: Re| r.into_response())
            .boxed()
    }
}


/// HTTP status codes — use with [`Unwarp::with_status`] or [`Unwarp::json_with_status`].
#[derive(Clone, Copy, Debug)]
pub enum Status {
    // 2xx
    Ok,
    Created,
    Accepted,
    NoContent,
    // 3xx
    MovedPermanently,
    Found,
    NotModified,
    TemporaryRedirect,
    PermanentRedirect,
    // 4xx
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    Conflict,
    Gone,
    UnprocessableEntity,
    TooManyRequests,
    // 5xx
    InternalServerError,
    NotImplemented,
    BadGateway,
    ServiceUnavailable,
    GatewayTimeout,
}

impl From<Status> for warp::http::StatusCode {
    fn from(s: Status) -> Self {
        match s {
            Status::Ok                  => warp::http::StatusCode::OK,
            Status::Created             => warp::http::StatusCode::CREATED,
            Status::Accepted            => warp::http::StatusCode::ACCEPTED,
            Status::NoContent           => warp::http::StatusCode::NO_CONTENT,
            Status::MovedPermanently    => warp::http::StatusCode::MOVED_PERMANENTLY,
            Status::Found               => warp::http::StatusCode::FOUND,
            Status::NotModified         => warp::http::StatusCode::NOT_MODIFIED,
            Status::TemporaryRedirect   => warp::http::StatusCode::TEMPORARY_REDIRECT,
            Status::PermanentRedirect   => warp::http::StatusCode::PERMANENT_REDIRECT,
            Status::BadRequest          => warp::http::StatusCode::BAD_REQUEST,
            Status::Unauthorized        => warp::http::StatusCode::UNAUTHORIZED,
            Status::Forbidden           => warp::http::StatusCode::FORBIDDEN,
            Status::NotFound            => warp::http::StatusCode::NOT_FOUND,
            Status::MethodNotAllowed    => warp::http::StatusCode::METHOD_NOT_ALLOWED,
            Status::Conflict            => warp::http::StatusCode::CONFLICT,
            Status::Gone                => warp::http::StatusCode::GONE,
            Status::UnprocessableEntity => warp::http::StatusCode::UNPROCESSABLE_ENTITY,
            Status::TooManyRequests     => warp::http::StatusCode::TOO_MANY_REQUESTS,
            Status::InternalServerError => warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            Status::NotImplemented      => warp::http::StatusCode::NOT_IMPLEMENTED,
            Status::BadGateway          => warp::http::StatusCode::BAD_GATEWAY,
            Status::ServiceUnavailable  => warp::http::StatusCode::SERVICE_UNAVAILABLE,
            Status::GatewayTimeout      => warp::http::StatusCode::GATEWAY_TIMEOUT,
        }
    }
}

/// A lightweight wrapper around `warp` that collects routes and serves them.
///
/// # Example
/// ```rust
/// use serde::Deserialize;
/// use warp::Rejection;
/// use unwarp::prelude::*;
///
/// #[derive(Deserialize)]
/// struct Payload { name: String }
///
/// #[tokio::main]
/// async fn main() {
///     let mut server = Unwarp::new();
///
///     server.route(
///         RouteBuilder::get("hello")
///             .handle(|| async { Ok::<_, Rejection>(warp::reply::html("Hello!")) })
///     );
///
///     server.route(
///         RouteBuilder::post("greet")
///             .json::<Payload>()
///             .handle(|p: Payload| async move {
///                 Ok::<_, Rejection>(warp::reply::json(&format!("Hi, {}!", p.name)))
///             })
///     );
///
///     server.serve(([127, 0, 0, 1], 3030)).await;
/// }
/// ```
pub struct Unwarp {
    routes: Vec<_Filter>,
}

impl Default for Unwarp {
    fn default() -> Self {
        Self::new()
    }
}

impl Unwarp {
    /// Create a new, empty unwarp instance.
    pub fn new() -> Self {
        Unwarp { routes: Vec::new() }
    }

    /// Register a [`RouteFilter`] produced by a route builder's `handle()` call.
    ///
    /// Returns `&mut Self` so calls can be chained.
    pub fn route(&mut self, route: _Filter) -> &mut Self {
        self.routes.push(route);
        self
    }

    /// Start serving all registered routes on `addr`.
    ///
    /// `addr` can be anything that converts into [`SocketAddr`], for example
    /// `([127, 0, 0, 1], 3030)` or `"0.0.0.0:8080".parse().unwrap()`.
    ///
    /// # Panics
    /// Panics if no routes have been registered.
    pub async fn serve(self, addr: impl Into<SocketAddr>) {
        let mut iter = self.routes.into_iter();
        let first = iter
            .next()
            .expect("Unwarp::serve: at least one route must be registered");

        // Fold remaining routes into a single composed filter via `or`.
        let combined = iter.fold(first.boxed(), |acc, next| {
            acc.or(next).unify().boxed()
        });

        warp::serve(combined).run(addr.into()).await;
    }

    /// Convenience constructor for a handler return value.
    ///
    /// Wraps `reply` with the given [`Status`] code and returns it as
    /// `Ok::<_, Rejection>(...)`, ready to be returned from a handler closure.
    ///
    /// # Example
    /// ```rust
    /// RouteBuilder::get("ping")
    ///     .handle(|| async {
    ///         Unwarp::with_status(Status::Ok, "online")
    ///     });
    /// ```
    pub fn with_status(status: Status, reply: impl Reply) -> Result<impl Reply, Rejection> {
        Ok::<_, Rejection>(warp::reply::with_status(reply, status.into()))
    }

    /// Convenience constructor for a JSON handler return value.
    ///
    /// Serialises `value` as JSON and returns it as `Ok::<_, Rejection>(...)`,
    /// ready to be returned from a handler closure.
    ///
    /// # Example
    /// ```rust
    /// RouteBuilder::post("echo")
    ///     .json::<Payload>()
    ///     .handle(|p: Payload| async move {
    ///         Unwarp::json(&p)
    ///     });
    /// ```
    pub fn json<T: Serialize>(value: &T) -> Result<impl Reply, Rejection> {
        Ok::<_, Rejection>(warp::reply::json(value))
    }

    /// Convenience constructor that serialises `value` as JSON **and** wraps it
    /// with the given [`Status`] code in a single call.
    ///
    /// Equivalent to `Unwarp::with_status(status, warp::reply::json(value))`.
    ///
    /// # Example
    /// ```rust
    /// RouteBuilder::post("users")
    ///     .json::<Payload>()
    ///     .handle(|p: Payload| async move {
    ///         Unwarp::json_with_status(Status::Created, &p)
    ///     });
    /// ```
    pub fn json_with_status<T: Serialize>(status: Status, value: &T) -> Result<impl Reply, Rejection> {
        Ok::<_, Rejection>(warp::reply::with_status(
            warp::reply::json(value),
            status.into(),
        ))
    }
}


#[macro_export]
/// Convenience macro with three forms:
///
/// - `unwarp!(status, json => value)` — serialise `value` as JSON and wrap with status.
/// - `unwarp!(status, reply)` — wrap any `warp::Reply` with status.
/// - `unwarp!(json_value)` — shorthand for `Unwarp::json(&json_value)`.
///
/// # Examples
/// ```rust
/// unwarp!(Status::Created, json => &my_struct)
/// unwarp!(Status::Ok, warp::reply::html("pong"))
/// unwarp!(my_struct)
/// ```
macro_rules! unwarp {
    ($status: expr, json => $json: expr) => {{
        $crate::Unwarp::with_status($status, $crate::warp::reply::json(&$json))
    }};

    ($status: expr, $reply: expr) => {{
        $crate::Unwarp::with_status($status, $reply)
    }};

    ($json: expr) => {{
        $crate::Unwarp::json(&$json)
    }};
}

pub mod prelude;