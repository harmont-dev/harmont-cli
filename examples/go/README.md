# Go example

Minimal Go module showing how to wire go build / test / vet / fmt into a Harmont CI pipeline. All four actions share one Go install.

## Run the pipeline

```sh
cd examples/go
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
