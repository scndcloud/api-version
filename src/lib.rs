#![feature(lazy_cell)]

use axum::{
    extract::{FromRequestParts, Request},
    http::{uri::PathAndQuery, HeaderName, HeaderValue, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::{
    headers::{self, Header},
    TypedHeader,
};
use regex::Regex;
use std::{
    fmt::{self, Debug, Display},
    sync::LazyLock,
};
use thiserror::Error;
use tracing::debug;

/// Create a `Versions` struct and a `FromRequestParts` such that it can be used as an extractor in
/// the provided `rewrite_api_version` function, which in turn can be used as axum
/// `from_fn`-middleware.
///
/// The first argument is the lower and the second the upper bound (both inclusive). Notice that
/// currently the related [XApiVersion] custom header only allows for versions from 0..+99.
///
/// # Example
///
/// - `versions![0, 1]` for versions `/v0` and `/v1`
#[macro_export]
macro_rules! versions {
    [$a:literal, $b:literal] => {
        #[derive(Debug)]
        pub struct Versions([Version; $b - $a + 1]);

        const VERSIONS: Versions =
            Versions(array_macro::array![n => $crate::Version::new(n + $a); $b - $a + 1]);

        impl AsRef<[$crate::Version]> for Versions {
            fn as_ref(&self) -> &[$crate::Version] {
                &self.0
            }
        }

        #[async_trait::async_trait]
        impl<S> axum::extract::FromRequestParts<S> for Versions {
            type Rejection = ();

            async fn from_request_parts(
                _parts: &mut axum::http::request::Parts,
                _state: &S,
            ) -> Result<Self, Self::Rejection> {
                Ok(VERSIONS)
            }
        }
    };
}

static X_API_VERSION: HeaderName = HeaderName::from_static("x-api-version");

/// Custom HTTP header conveying the API version, which is expected to be a version designator
/// starting with 'v' followed by a number from 0..+99 without leading zero.
#[derive(Debug)]
pub struct XApiVersion(Version);

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
            .and_then(|s| Version::try_from(s).ok())
            .map(XApiVersion)
            .ok_or_else(headers::Error::invalid)
    }

    fn encode<E: Extend<HeaderValue>>(&self, _values: &mut E) {
        // We do not yet need to encode this header.
        panic!("not yet implemented");
    }
}

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^v(0|[1-9][0-9]?)$"#).expect("version regex is valid"));

/// Version designator starting with 'v' followed by a number from 0..+99 without leading zero.
///
/// # Examples
///
/// - v0
/// - v1
/// - v10
/// - v99
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(usize);

impl Version {
    pub const fn new(value: usize) -> Self {
        Self(value)
    }
}

impl TryFrom<String> for Version {
    type Error = VersionError;

    fn try_from(version: String) -> Result<Self, Self::Error> {
        match VERSION.captures(&version) {
            Some(c) => Ok(Version(
                c.get(1)
                    .expect("version match has a first capture group")
                    .as_str()
                    .parse()
                    .expect("version can be parsed as usize"),
            )),
            None => Err(VersionError(version)),
        }
    }
}

impl TryFrom<&str> for Version {
    type Error = VersionError;

    fn try_from(version: &str) -> Result<Self, Self::Error> {
        version.to_owned().try_into()
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[derive(Debug, Error)]
#[error("invalid version: {0}")]
pub struct VersionError(String);

/// Function to be used as axum middleware to rewrite request paths from optionally carrying a
/// single [XApiVersion] custom header to paths with respective prefixes, e.g. "/v0/<path>". If a
/// request has no [XApiVersion] custom header the latest (highest) version is used. The readiness
/// probe "/" is not rewritten.
///
/// Paths must not start with a version prefix, e.g. "/v0".
pub async fn rewrite_api_version<V, S>(
    versions: V,
    x_api_version: Option<TypedHeader<XApiVersion>>,
    mut request: Request,
    next: Next,
) -> Response
where
    V: AsRef<[Version]> + FromRequestParts<S>,
{
    // Always serve "/", typically used as readiness probe, unmodified.
    if request.uri().path() == "/" {
        return next.run(request).await;
    }

    let versions = versions.as_ref();

    // Do not allow the path to start with one of the valid version prefixes.
    // Implementation note: we use [1..] to skip the leading '/'.
    if versions
        .iter()
        .any(|version| request.uri().path()[1..].starts_with(&version.to_string()))
    {
        let response = (
            StatusCode::BAD_REQUEST,
            "path must not start with version prefix like '/v0'",
        );
        return response.into_response();
    }

    // Determine API version.
    let version = x_api_version
        .as_ref()
        .map(|TypedHeader(XApiVersion(v))| v)
        .unwrap_or_else(|| versions.last().expect("versions is not empty"));
    if !versions.contains(version) {
        let response = (
            StatusCode::NOT_FOUND,
            format!("unknown version '{version}'"),
        );
        return response.into_response();
    }
    debug!(?version, "using API version");

    // Prepend the suitable prefix to the request URI.
    let mut parts = request.uri().to_owned().into_parts();
    let paq = parts.path_and_query.expect("uri has 'path and query'");
    let mut paq_parts = paq.as_str().split('?');
    let path = paq_parts.next().expect("uri has path");
    let paq = match paq_parts.next() {
        Some(query) => format!("/{version}{path}?{query}"),
        None => format!("/{version}{path}"),
    };
    let paq = PathAndQuery::from_maybe_shared(paq).expect("new 'path and query' is valid");
    parts.path_and_query = Some(paq);
    let uri = Uri::from_parts(parts).expect("parts are valid");

    // Rewrite the request URI and run the downstream services.
    request.uri_mut().clone_from(&uri);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware, routing::get, Router};
    use futures::{future::ok, TryStreamExt};
    use std::iter::Extend;
    use tower::{Layer, Service};

    #[tokio::test]
    async fn test() {
        versions!(0, 1);

        let app = Router::new()
            .route("/", get(ok_0))
            .route("/v0/test", get(ok_0))
            .route("/v1/test", get(ok_1));
        let mut app = middleware::from_fn(rewrite_api_version::<Versions, ()>).layer(app);

        // Verify that for the root path (health check) versions don't matter.
        let request = Request::builder()
            .uri("/")
            .header(&X_API_VERSION, "v99")
            .body(Body::empty())
            .unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // No version.
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, "1");

        // Existing version.
        let request = Request::builder()
            .uri("/test")
            .header(&X_API_VERSION, "v0")
            .body(Body::empty())
            .unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, "0");

        // Another existing version.
        let request = Request::builder()
            .uri("/test")
            .header(&X_API_VERSION, "v1")
            .body(Body::empty())
            .unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, "1");

        // Non-existing version.
        let request = Request::builder()
            .uri("/test")
            .header(&X_API_VERSION, "v2")
            .body(Body::empty())
            .unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Invalid path starting with existing version prefix.
        let request = Request::builder()
            .uri("/v0x")
            .header(&X_API_VERSION, "v2")
            .body(Body::empty())
            .unwrap();
        let response = app.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    async fn ok_0() -> impl IntoResponse {
        "0"
    }

    async fn ok_1() -> impl IntoResponse {
        "1"
    }

    async fn text(response: Response) -> String {
        let text = response
            .into_body()
            .into_data_stream()
            .try_fold(vec![], |mut acc, bytes| {
                acc.extend(bytes);
                ok(acc)
            })
            .await;
        assert!(text.is_ok());
        let text = String::from_utf8(text.unwrap()).unwrap();
        text
    }
}
