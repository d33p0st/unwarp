# unwarp

A minimal, ergonomic wrapper around [`warp`](https://github.com/seanmonstar/warp) — define routes, attach handlers, and serve. No boilerplate, no filter chains.

[![Crates.io](https://img.shields.io/crates/v/unwarp)](https://crates.io/crates/unwarp)
[![docs.rs](https://docs.rs/unwarp/badge.svg)](https://docs.rs/unwarp)
[![GitHub](https://img.shields.io/badge/github-d33p0st%2Funwarp-8da0cb?logo=github)](https://github.com/d33p0st/unwarp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## Overview

`unwarp` sits on top of `warp` and hides the filter-composition ceremony behind a simple builder API:

1. Pick an HTTP method and path with `RouteBuilder`.
2. Optionally specify a JSON body or query-string type.
3. Attach an async handler with `.handle(...)` — you get back a type-erased `_Filter`.
4. Register every route on an `Unwarp` instance with `.route(...)`.
5. Call `.serve(addr).await` to start listening.

---

## Installation

```toml
[dependencies]
unwarp = "1.2.1"
```

---

## Quick-start

```rust
use unwarp::prelude::*;
use warp::Rejection;

#[tokio::main]
async fn main() {
    let mut server = Unwarp::new();

    server.route(
        RouteBuilder::get("ping")
            .handle(|| async {
                Ok::<_, Rejection>(warp::reply::html("pong"))
            })
    );

    server.serve(([127, 0, 0, 1], 3030)).await;
}
```

---

## Route builders

### Plain route (no body / no query params)

```rust
RouteBuilder::get("hello")
    .handle(|| async {
        Ok::<_, Rejection>(warp::reply::html("Hello, world!"))
    });
```

### JSON body

Declare the expected body type with `.json::<T>()`. `T` must implement `serde::DeserializeOwned`.

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUser { name: String, email: String }

RouteBuilder::post("users")
    .json::<CreateUser>()
    .handle(|body: CreateUser| async move {
        Ok::<_, Rejection>(warp::reply::json(&body.name))
    });
```

### Query parameters

Declare the query-string type with `.query::<T>()`. `T` must implement `serde::DeserializeOwned`.

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Search { q: String, limit: Option<u32> }

RouteBuilder::get("search")
    .query::<Search>()
    .handle(|params: Search| async move {
        Ok::<_, Rejection>(warp::reply::json(&params.q))
    });
```

### Supported HTTP methods

| Builder constructor        | HTTP method |
|----------------------------|-------------|
| `RouteBuilder::get(path)`     | GET         |
| `RouteBuilder::post(path)`    | POST        |
| `RouteBuilder::put(path)`     | PUT         |
| `RouteBuilder::delete(path)`  | DELETE      |
| `RouteBuilder::patch(path)`   | PATCH       |
| `RouteBuilder::head(path)`    | HEAD        |
| `RouteBuilder::options(path)` | OPTIONS     |

---

## Constructing responses

`Unwarp` ships two static helpers so you rarely need to write `Ok::<_, Rejection>(...)` yourself.

### `Unwarp::with_status`

```rust
RouteBuilder::delete("items/42")
    .handle(|| async {
        Unwarp::with_status(Status::NoContent, warp::reply())
    });
```

### `Unwarp::json`

```rust
RouteBuilder::get("config")
    .handle(|| async {
        Unwarp::json(&serde_json::json!({ "version": "1.0" }))
    });
```

### `unwarp!` macro (prelude)

The `unwarp!` macro is a shorthand for the two helpers above:

```rust
// Serialise value as JSON and wrap with a status code
unwarp!(Status::Created, json => &item)

// Wrap any warp::Reply (html, plain text, etc.) with a status code
unwarp!(Status::Ok, warp::reply::html("pong"))

// Shorthand for Unwarp::json(&value)
unwarp!(my_struct)
```

> **Why `json =>`?** Both `unwarp!(status, json_val)` and `unwarp!(status, string_val)` look identical to the macro engine — it matches on token structure, not types. The `json =>` keyword discriminator makes the two arms syntactically distinct so the right one is always chosen.

---

## `Status` enum

A typed representation of common HTTP status codes, convertible to `warp::http::StatusCode` via `Into`.

| Variant                      | Code |
|------------------------------|------|
| `Status::Ok`                 | 200  |
| `Status::Created`            | 201  |
| `Status::Accepted`           | 202  |
| `Status::NoContent`          | 204  |
| `Status::MovedPermanently`   | 301  |
| `Status::Found`              | 302  |
| `Status::NotModified`        | 304  |
| `Status::TemporaryRedirect`  | 307  |
| `Status::PermanentRedirect`  | 308  |
| `Status::BadRequest`         | 400  |
| `Status::Unauthorized`       | 401  |
| `Status::Forbidden`          | 403  |
| `Status::NotFound`           | 404  |
| `Status::MethodNotAllowed`   | 405  |
| `Status::Conflict`           | 409  |
| `Status::Gone`               | 410  |
| `Status::UnprocessableEntity`| 422  |
| `Status::TooManyRequests`    | 429  |
| `Status::InternalServerError`| 500  |
| `Status::NotImplemented`     | 501  |
| `Status::BadGateway`         | 502  |
| `Status::ServiceUnavailable` | 503  |
| `Status::GatewayTimeout`     | 504  |

---

## Full example

```rust
use serde::{Deserialize, Serialize};
use unwarp::prelude::*;
use warp::Rejection;

#[derive(Deserialize, Serialize)]
struct Message { text: String }

#[derive(Deserialize)]
struct Filter { prefix: Option<String> }

#[tokio::main]
async fn main() {
    let mut server = Unwarp::new();

    // GET /ping
    server.route(
        RouteBuilder::get("ping")
            .handle(|| async {
                Unwarp::with_status(Status::Ok, warp::reply::html("pong"))
            })
    );

    // POST /echo  — JSON body
    server.route(
        RouteBuilder::post("echo")
            .json::<Message>()
            .handle(|msg: Message| async move {
                Unwarp::json(&msg)
            })
    );

    // GET /search?prefix=foo  — query params
    server.route(
        RouteBuilder::get("search")
            .query::<Filter>()
            .handle(|f: Filter| async move {
                let result = f.prefix.unwrap_or_default();
                Unwarp::json(&Message { text: result })
            })
    );

    server.serve(([0, 0, 0, 0], 3030)).await;
}
```

---

## Prelude

Import everything you need in one line:

```rust
use unwarp::prelude::*;
// re-exports: Unwarp, unwarp!, RouteBuilder, Status
```

---

## License

MIT — see [LICENSE](LICENSE).
