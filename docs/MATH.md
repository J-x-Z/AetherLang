# CPU tensors and reverse-mode differentiation

The supported numerical path is `stdlib/tensor.aeth` plus `stdlib/autodiff.aeth`.
All shape handling, indexing, allocation ownership, matrix multiplication and
reverse propagation execute as Aether code. Scalar `tanh` uses the host math
library. This is a bounded CPU implementation, not JAX API or GPU compatibility.

`Tensor` holds f64 elements with rank 0 through 4. Shapes, strides and offsets
are checked before allocation or view construction; shape products cannot wrap.
Rank 0 has one element. Zero-length dimensions are valid. Arithmetic is
same-shape add/subtract/multiply; matmul accepts rank-2 compatible dimensions.
Transpose and positive-step, half-open slices produce shared-storage views.
Operations read logical row-major indices, including non-contiguous inputs.
Broadcasting, negative strides, dynamic ranks, BLAS and GPU execution are not
implemented by this API.

Every successful tensor constructor, clone, transpose or slice returns one
owning `*Tensor` handle. Call `tensor_free` exactly once per handle. A view retains
its storage and remains usable after its parent handle is released. Pointer
copies do not retain ownership. `tensor_clone` copies the logical values into
independent contiguous storage. Views share mutations. Library fields are
invariants; do not modify them directly or access a handle after release. This
manual boundary does not claim automatic ownership checking for raw pointers.

Fallible operations return `Result<T, i32>`: 1 means incompatible/invalid shape,
2 means allocation failure, 3 means out-of-bounds/null data, and 4 means size or
stride overflow. Except the documented nullable `tensor_free`, pass live handles.
Allocation failure rolls back newly allocated state. No operation intentionally
aborts the process on a shape or allocation error.

Create a tape with `ad_new`, add inputs with `ad_variable`, combine node IDs with
`ad_add`, `ad_sub`, `ad_mul`, `ad_matmul`, `ad_tanh` and `ad_sum`, then call
`ad_backward` on a rank-0 loss. Variables copy input data so later source
mutation cannot change the recorded forward pass. Values and gradients returned
by `ad_value` and `ad_gradient` are borrowed handles: do not free or mutate them;
clone if an independent value must outlive the tape. `ad_free` frees all nodes.
Backward resets gradients, follows only ancestors of the selected loss, and
accumulates every incoming edge, including repeated operands. Invalid node IDs
return error 10; a non-scalar loss returns 11. Failed recording leaves the
previous tape intact. There is no automatic tape retention across backward or
higher-order differentiation.

Migration: the former generic `Tensor<T, const N>`/`Vector` declarations and
scalar `Value` tape were not an executable equivalent of these guarantees.
Use the concrete f64 handle API above. The `autograd` module now re-exports
`autodiff`; it contains no second engine. `matrix` is a rank-2 convenience layer
using tensor storage and operations (`matrix_zeros`, `matrix_mul`,
`matrix_transpose`, logical-index `matrix_get`/`matrix_set`, `matrix_free`). The
old f32 `Matrix4x4`/`Vec3`/`Vec4` API is removed. Old generated tensor, matrix and
autograd C snapshots are removed from the source tree and remain in Git history.

Run the reproducible workload with an external work directory:

```sh
python3 tests/math_values.py --compiler /path/to/aethc --work-dir /external/math
```

It compares C and LLVM at O0/O2, checks retained non-contiguous views and shape
errors, differentiates every parameter of a matmul/tanh/squared-loss expression
against finite differences, and runs 240 gradient-descent steps. A separate
allocator shim injects failure at each allocation boundary and checks that live
allocation counts return to zero. C and LLVM emitted failure workloads run with
AddressSanitizer/UndefinedBehaviorSanitizer; the numerical C workload also runs
with both sanitizers. The shim only controls allocation; it implements no math.

## Native math consumers

`stdlib/blas.aeth` binds LP64 CBLAS with 32-bit dimensions and C `size_t`
indices for raw IAMAX. Its OpenBLAS consumer checks a 2×3 by 3×2 matrix
product, vector updates and a double-precision dot product.

On macOS, `runtime/metal/bridge.m` exposes retained Metal handles. Release
them with `aether_mtl_release`. The pointer from `aether_mtl_buffer_contents`
is borrowed for the lifetime of its buffer. `metal_sync` returns the command
completion status. The tested Aether consumer allocates a shared buffer,
compiles a Metal kernel, dispatches it and checks every one of 257 results
before releasing its resources. The output includes the device registry ID.

```sh
python3 tests/ffi_modules.py --compiler "$AETHC" --work-dir "$WORK/native-sdks" \
  --backends c llvm --metal --openblas "$OPENBLAS_LIBRARY"
```

CUDA-Q uses its own SDK backend. Aether's Metal compute interface runs a Metal
kernel directly. Shared operator scheduling, quantum circuits on Metal and
Adreno execution are tracked in T21–T24.
