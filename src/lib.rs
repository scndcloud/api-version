#![feature(lazy_cell)]

use axum::{
    extract::Request,
    http::{uri::PathAndQuery, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    RequestExt,
};
use axum_extra::{
    headers::{self, Header},
    TypedHeader,
};
use futures::future::BoxFuture;
use regex::Regex;
use std::{
    fmt::Debug,
    sync::LazyLock,
    task::{Context, Poll},
};
use thiserror::Error;
use tower::{Layer, Service};
use tracing::debug;

/// Create an [ApiVersionLayer] correctly initialized with non-empty and strictly monotonically
/// increasing version in the given inclusive range.
#[macro_export]
macro_rules! api_version {
    ($from:literal, $to:literal) => {
        {
            let versions = array_macro::array![n => n as u16 + $from; $to - $from + 1];
            $crate::ApiVersionLayer::new(versions).expect("versions are valid")
        }
    };
}

/// Axum middleware to add a version prefix to a request's path based on a set of versions and an
/// optional [XApiVersion] header. If no such header is present, the highest version is used.
///
/// The readiness probe "/" is not rewritten.
///
/// Paths must not start with a version prefix, e.g. "/v0".
#[derive(Clone)]
pub struct ApiVersionLayer<const N: usize> {
    versions: [u16; N],
}

impl<const N: usize> ApiVersionLayer<N> {
    /// Create a new [RewriteVersionLayer].
    ///
    /// The given versions must not be empty and must be strictly monotonically increasing, e.g.
    /// `[0, 1, 2]`.
    pub fn new(versions: [u16; N]) -> Result<Self, NewApiVersionLayerError> {
        if versions.is_empty() {
            return Err(NewApiVersionLayerError::Empty);
        }

        if versions.as_slice().windows(2).any(|w| w[0] >= w[1]) {
            return Err(NewApiVersionLayerError::NotIncreasing);
        }

        Ok(Self { versions })
    }
}

#[derive(Debug, Error)]
pub enum NewApiVersionLayerError {
    #[error("versions must not be empty")]
    Empty,

    #[error("versions must be strictly monotonically increasing")]
    NotIncreasing,
}

impl<const N: usize, S> Layer<S> for ApiVersionLayer<N> {
    type Service = ApiVersion<N, S>;

    fn layer(&self, inner: S) -> Self::Service {
        ApiVersion {
            inner,
            versions: self.versions,
        }
    }
}

/// See [ApiVersionLayer].
#[derive(Clone)]
pub struct ApiVersion<const N: usize, S> {
    inner: S,
    versions: [u16; N],
}

impl<const N: usize, S> Service<Request> for ApiVersion<N, S>
where
    S: Service<Request, Response = Response> + Send + 'static + Clone,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let versions = self.versions;

        Box::pin(async move {
            // Always serve "/", typically used as readiness probe, unmodified.
            if request.uri().path() == "/" {
                return inner.call(request).await;
            }

            // Do not allow the path to start with one of the valid version prefixes.
            if versions
                .iter()
                .any(|version| request.uri().path().starts_with(&format!("/v{version}")))
            {
                let response = (
                    StatusCode::BAD_REQUEST,
                    "path must not start with version prefix like '/v0'",
                );
                return Ok(response.into_response());
            }

            // Determine API version.
            let version = request.extract_parts::<TypedHeader<XApiVersion>>().await;
            let version = version
                .as_ref()
                .map(|TypedHeader(XApiVersion(v))| v)
                .unwrap_or_else(|_| versions.last().expect("versions is not empty"));
            if !versions.contains(version) {
                let response = (
                    StatusCode::NOT_FOUND,
                    format!("unknown version '{version}'"),
                );
                return Ok(response.into_response());
            }
            debug!(?version, "using API version");

            // Prepend the suitable prefix to the request URI.
            let mut parts = request.uri().to_owned().into_parts();
            let paq = parts.path_and_query.expect("uri has 'path and query'");
            let mut paq_parts = paq.as_str().split('?');
            let path = paq_parts.next().expect("uri has path");
            let paq = match paq_parts.next() {
                Some(query) => format!("/v{version}{path}?{query}"),
                None => format!("/v{version}{path}"),
            };
            let paq = PathAndQuery::from_maybe_shared(paq).expect("new 'path and query' is valid");
            parts.path_and_query = Some(paq);
            let uri = Uri::from_parts(parts).expect("parts are valid");

            // Rewrite the request URI and run the downstream services.
            request.uri_mut().clone_from(&uri);
            inner.call(request).await
        })
    }
}

/// Header name for the [XApiVersion] custom HTTP header.
pub static X_API_VERSION: HeaderName = HeaderName::from_static("x-api-version");

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^v(0|[1-9][0-9]?)$"#).expect("version regex is valid"));

/// Custom HTTP header conveying the API version, which is expected to be a version designator
/// starting with `'v'` followed by a number from 0..+99 without leading zero, e.g. `v0`.
#[derive(Debug)]
pub struct XApiVersion(u16);

impl Header for XApiVersion {
    fn name() -> &'static HeaderName {
        &X_API_VERSION
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue>,
    {
        values
            .next()
            .and_then(|v| v.to_str().ok())
            .and_then(|s| VERSION.captures(s).and_then(|c| c.get(1)))
            .and_then(|m| m.as_str().parse().ok())
            .map(XApiVersion)
            .ok_or_else(headers::Error::invalid)
    }

    fn encode<E: Extend<HeaderValue>>(&self, _values: &mut E) {
        // We do not yet need to encode this header.
        unimplemented!("not yet needed");
    }
}
