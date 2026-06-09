# C++ example

CMake + CTest hello-add C++ library. Pipeline uses `hm.cmake(lang="cpp")` to label steps `:cpp:` and runs configure + build + ctest + clang-format check.

## Run the pipeline

```sh
cd examples/cpp
hm run ci --local
```

See `.hm/pipeline.py` for the definition; `examples/README.md` for the full index.
