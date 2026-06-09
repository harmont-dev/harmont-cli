# Zig example

`zig init`-style project with `build.zig`, root library, and a test block. Pipeline downloads Zig 0.13.0 and runs `zig build / build test / fmt --check .`.

## Run the pipeline

```sh
cd examples/zig
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
