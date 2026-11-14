# Quantum and accelerator boundary

`stdlib/quantum.aeth` owns circuit construction, gate validation, a complex128
CPU state-vector simulator, Pauli expectations and parameter-shift derivatives.
H/X/Y/Z/Rx/Ry/Rz/CX/CZ/SWAP compose normally; qubit q is bit q of the amplitude
index. Large finite rotation angles use a separate same-axis shift rotation so
adding pi/2 cannot vanish through floating-point rounding. The reference state
allocator has a caller budget and a 24-qubit hard ceiling (256 MiB for amplitudes).

`stdlib/quantum_cudaq.aeth` is the optional native backend. Aether passes its gate
records and owned state buffer through a versioned C ABI. The small C++ adapter
uses CUDA-Q's dynamic kernel builder and copies the result; it does not implement
the optimizer or substitute a prewritten example circuit. SDK exceptions become
Result errors; SDK objects and C++ types do not cross the ABI. Calls are serialized
around the SDK. Only fp64 state-vector backends are accepted by this adapter.

CUDA-Q on macOS ARM64 uses CPU simulation. This is an officially supported mode;
**there is no Metal/Apple-GPU backend in this integration**. NVIDIA GPU execution
is validated separately on Linux. See the [official platform requirements](https://nvidia.github.io/cuda-quantum/latest/using/quick_start.html)
and [dynamic kernel builder API](https://nvidia.github.io/cuda-quantum/latest/specification/cudaq/dynamic_kernels.html).

Build against an existing C++ SDK (no implicit download or global installation):

```sh
python3 scripts/build_cudaq.py --sdk /path/to/cudaq \
  --backend qpp --output /external/work/libaether_cudaq.dylib
python3 tests/cudaq_runtime.py --compiler /path/to/aethc \
  --bridge /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-test
python3 tests/cudaq_boundaries.py --compiler /path/to/aethc \
  --library /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-boundaries
```

Linux uses `.so`; select `--backend custatevec-fp64` and make matching CUDA and
cuStateVec libraries available to both the linker and loader. `--runtime-path`
sets rpaths in the adapter; an SDK's transitive dependencies may additionally
need `LD_LIBRARY_PATH` / a linker rpath-link. Pass `--gpu` to the validation runner;
it asserts actual state placement (`is_on_gpu`), not merely a requested label.

For an existing wheel runtime, matching source headers can be supplied with
`--source /path/to/cuda-quantum-source --include /path/to/dependencies`. This run
used CUDA-Q 0.16.0 on Apple ARM64 and 0.16.0.post1 plus cuStateVec 1.15.1 on Linux
with an RTX 3060. No nvq++ installation or shell profile modification was needed.
The adapter uses a normal C++20 compiler and the SDK's JIT builder.

The validation circuit combines all ten gate kinds on three qubits, checks
complex amplitudes against Aether's own simulator for eight parameter sets, and
runs a 40-step Aether optimization loop with SDK evaluations. Both tested devices
reached energy -0.999999575958. These are correctness observations, not GPU speedup
measurements. CPU qpp tensor storage and GPU to_host use logical little-endian
wire order; the adapter deliberately avoids CUDA-Q's big-endian display index
operator when copying amplitudes.

`stdlib/cuda.aeth` separately exposes the CUDA runtime and cuBLAS C ABI with
fallible owned buffer/stream/handle helpers. It is not CUDA-Q and does not claim
Aether device-kernel code generation. Raw GPU pointers and asynchronous lifetimes
remain under caller control. The mathematical layer currently provides CPU
f64 tensors/autodiff; it is not a JAX API implementation, a tensor JIT compiler,
or an automatic differentiator through arbitrary CUDA-Q kernels.

On a CUDA host, reproduce the native cuBLAS checks with:

```sh
python3 tests/cuda_native.py --compiler /path/to/aethc \
  --work-dir /external/work/cuda-native --backends c llvm \
  --cuda-lib-dir /opt/cuda/lib64
```

The optional library directory supplies full shared-library paths to the
compiler's `--link-object` option and updates the child process's loader search
path. Without it, the runner uses `--link-library cublas --link-library cudart`
and the host's configured library paths. Every build and execution is recorded
in `results.json`. Exit 77 means no CUDA device: the runner reports a skip and
returns 77, rather than counting it as a successful GPU run. Missing libraries,
other runtime errors and numerical failures are failures.

Linux x86_64 with an RTX 3060 passed both C and LLVM at O0/O2 for this workload:
17×13 output matrices with reduction dimension 19, nonzero alpha/beta, and both
SGEMM and DGEMM. Aether initializes the inputs, computes the reference and checks
every output; cuBLAS performs the device computation. All four runs reported
`sgemm-max=0 dgemm-max=0` for these exact test inputs. The separate CUDA-Q GPU
workload also passed C/LLVM O0/O2 with `on_gpu=1`. These results establish the
tested native interoperability, not arbitrary target ABI coverage or a speedup.
