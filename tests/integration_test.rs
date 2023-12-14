use api_version::{ApiVersionFilter, ApiVersionLayer, X_API_VERSION};
use axum::{
    body::Body,
    http::{Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures::{future::ok, TryStreamExt};
use std::iter::Extend;
use tower::{Layer, Service};

#[tokio::test]
async fn test() {
    let app = Router::new()
        .route("/", get(ok_0))
        .route("/v0/test", get(ok_0))
        .route("/v1/test", get(ok_1))
        .route("/foo", get(ok_foo));
    let mut app = ApiVersionLayer::new([0, 1], FooFilter).unwrap().layer(app);

    // Verify that filter is working.
    let request = Request::builder().uri("/foo").body(Body::empty()).unwrap();
    let response = app.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "foo");

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

#[derive(Clone)]
struct FooFilter;

impl ApiVersionFilter for FooFilter {
    async fn filter(&self, uri: &Uri) -> bool {
        !uri.path().starts_with("/foo")
    }
}

async fn ok_0() -> impl IntoResponse {
    "0"
}

async fn ok_1() -> impl IntoResponse {
    "1"
}

async fn ok_foo() -> impl IntoResponse {
    "foo"
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
