# api-version

[![license][license-badge]][license-url]
[![build][build-badge]][build-url]

[license-badge]: https://img.shields.io/github/license/scndcloud/api-version
[license-url]: https://github.com/scndcloud/api-version/blob/main/LICENSE
[build-badge]: https://img.shields.io/github/actions/workflow/status/scndcloud/api-version/ci.yaml
[build-url]: https://github.com/scndcloud/api-version/actions/workflows/ci.yaml

Axum middleware to add a version prefix to a request's path based on a set of versions and an optional `x-api-version` header.

The custom `x-api-version` HTTP header is conveying the API version, which is expected to be a version designator starting with `'v'` followed by a number from 0..+99 without leading zero, e.g. `v0`.

If no such header is present, the highest version is used.

The readiness probe `"/"` is not rewritten.

Paths must not start with a version prefix, e.g. `"/v0"`.

## Example

```rust
use api_version::rewrite_versions;

let app = Router::new()
    .route("/", get(ok_0))
    .route("/v0/test", get(ok_0))
    .route("/v1/test", get(ok_1));
let app = rewrite_versions!(0, 1).layer(app);
```

## License ##

This code is open source software licensed under the [Apache 2.0 License](http://www.apache.org/licenses/LICENSE-2.0.html).
