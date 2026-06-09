# Python (uv) example

uv-managed Python library with pytest + ruff + mypy. Pipeline shares one uv install across test / lint / fmt / typecheck.

## Run the pipeline

```sh
cd examples/python-uv
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
