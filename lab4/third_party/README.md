# Lab4 Third-Party Dependencies

This directory is reserved for third-party source pointers.

Use a Git submodule for `llama.cpp`:

```bash
git submodule add https://github.com/ggml-org/llama.cpp lab4/third_party/llama.cpp
git submodule update --init --recursive
```

Do not copy vendored source snapshots into this directory manually. Do not
commit build outputs.

The upstream `llama.cpp` repository contains its own `Makefile`. It is not
authored or tracked as a regular file by the parent DiPECS repository, and this
project builds the dependency with CMake only. If the course checker forbids
any recursively visible `Makefile`, keep the `llama.cpp` checkout outside this
repository and point `lab4-bench --executable` to that external path instead.
