# Rust example

Cargo library crate showing how to wire cargo + clippy + rustfmt into a Harmont CI pipeline. Build + test + clippy + fmt, all sharing one rustup install.

## Run the pipeline

```sh
cd examples/rust
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
