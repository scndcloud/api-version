# api-version

Axum middleware to rewrite requests from optionally carrying an `x-api-version` header to paths with respective prefixes.

In order to use this crate, the following dependencides are also needed (check the exact versions in this `Cargo.toml`):
- `array-macro = { version = "2.1" }`
- `async-trait = { version = "0.1" }`
