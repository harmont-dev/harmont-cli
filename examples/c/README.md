# C example

CMake + CTest hello-add C library. Pipeline runs cmake configure + build + ctest + clang-format check.

## Run the pipeline

```sh
cd examples/c
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
