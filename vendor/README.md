# vendor

Patched copies of crates.io crates, wired in by `[patch.crates-io]` in the root
`Cargo.toml`. Each is the published crate with the change below and nothing
else, so it can be diffed against the `.crate` from crates.io.

## dicebear-core 11.0.0-rc.2

Mirage Mockery draws a card with no art from a DiceBear style. The published
crate depends on `jsonschema = "0.46"` with its default features, which are
`resolve-http`, `resolve-file` and `tls-aws-lc-rs`: reqwest, hyper, rustls and
aws-lc-sys, about a hundred crates, for fetching remote schemas DiceBear never
references (its schemas are embedded by `dicebear-schema`). aws-lc-sys does
not build for `wasm32-unknown-unknown`, and `mockery-core` has to. The only
change is `default-features = false` on that dependency in `Cargo.toml`, and
dropping the `[[test]]` targets, whose files the published crate ships but
which are not copied here.

Remove this copy and the patch when an upstream release depends on jsonschema
without its default features (upstream: `src/rust/core/Cargo.toml` on the
`11.x` branch of github.com/dicebear/dicebear).
