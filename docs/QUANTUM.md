# Quantum and accelerator boundary

`stdlib/quantum.aeth` owns circuit construction, gate validation, a complex128
CPU state-vector simulator, Pauli expectations and parameter-shift derivatives.
H/X/Y/Z/Rx/Ry/Rz/CX/CZ/SWAP compose normally; qubit q is bit q of the amplitude
index. Large finite rotation angles use a separate same-axis shift rotation so
adding pi/2 cannot vanish through floating-point rounding. The reference state
allocator has a caller budget and a 24-qubit hard ceiling (256 MiB for amplitudes).

`stdlib/quantum_cudaq.aeth` is an optional adapter to NVIDIA's CUDA-Q SDK.
Aether builds the circuit and runs the classical optimizer. Its version 1 C ABI
sends gate records to a C++ adapter, which creates an SDK kernel, applies the
requested gates and invokes `cudaq::get_state`. The adapter requires fp64,
copies amplitudes into an Aether-owned host buffer and returns the SDK state's
original CPU/GPU placement. Calls are serialized and SDK exceptions become
`Result` errors. The interface covers pure-state simulation for the ten gates
above. Arbitrary CUDA-Q kernels, physical QPU submission and multi-GPU
selection are outside this interface. [CUDA-Q dynamic kernel builder](https://nvidia.github.io/cuda-quantum/latest/specification/cudaq/dynamic_kernels.html)

| Host and device | Execution path | Verified workload |
| --- | --- | --- |
| macOS arm64, Apple M4 CPU | CUDA-Q qpp, fp64 | Mixed circuits and a 16-qubit, 65,536-amplitude comparison, `on_gpu=0` |
| Linux x86_64, RTX 3060 Laptop GPU | CUDA-Q cuStateVec, fp64 | Eight mixed circuits and 40 optimizer steps, `on_gpu=1` |
| macOS arm64, Apple M4 GPU | Aether compute queue, Metal, complex64 | 16 qubits, all amplitudes checked, 42 submissions |
| Linux x86_64, RTX 3060 Laptop GPU | Aether compute queue, Vulkan, complex64 | Same 16-qubit workload and 42 submissions |
| Windows 11 x64, RTX 4060 Laptop GPU | Aether compute queue, Vulkan, complex64 | C and LLVM, 65,536 amplitudes, 42 submissions, max L1 error per amplitude 7.81873961e-8 |
| Android, Adreno 750 | Aether compute queue, Vulkan, complex64 | Same 16-qubit workload and 42 submissions |

CUDA-Q documents GPU simulation targets on Linux and CPU simulation on Apple
Silicon. On Windows its documented installation route is Linux under WSL 2.
The table records the tested host/device combinations. Native Windows and WSL 2
CUDA-Q execution are unverified in this project. The Metal and Adreno rows run
Aether's own simulator and require no CUDA-Q SDK. [CUDA-Q installation](https://nvidia.github.io/cuda-quantum/latest/using/install/local_installation.html) · [CUDA-Q simulator targets](https://nvidia.github.io/cuda-quantum/latest/using/quick_start.html)

On Windows the optional compute package contains `aether_vulkan.dll`,
`loader.obj` and C/LLVM workloads. Run
`python windows_vulkan.py run --dir . --vendor 10de` from its extracted directory to check NVIDIA device selection,
the CPU reference, the provider ABI, and the quantum result. An explicit
provider path such as `.\aether_vulkan.dll` selects the library. Source build
options are available through `python tests/windows_vulkan.py build --help`.

Build against an existing C++ SDK (no implicit download or global installation):

```sh
python3 scripts/build_cudaq.py --sdk /path/to/cudaq \
  --backend qpp --output /external/work/libaether_cudaq.dylib
/path/to/aethc build examples/quantum/cudaq_device.aeth --sysroot . \
  --link-object /external/work/libaether_cudaq.dylib -o /external/work/cudaq-device
/external/work/cudaq-device
python3 tests/cudaq_runtime.py --compiler /path/to/aethc \
  --bridge /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-test
python3 tests/cudaq_boundaries.py --compiler /path/to/aethc \
  --library /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-boundaries
```

The probe prepares X|0⟩, checks both complex amplitudes, and prints
`state_location=cpu` or `state_location=gpu` from CUDA-Q's `is_on_gpu()`.
Use `cudaq-device --require gpu` when a CPU result must fail. This probes the
linked simulator and reports placement for that execution. It does not enumerate
adapters or return a GPU model or ordinal.

The build helper selects the SDK simulator library at link time. Linux uses
`.so`. Select `--backend custatevec-fp64` and make matching CUDA and cuStateVec
libraries available to both the linker and loader. `--runtime-path` sets
rpaths in the adapter. Transitive dependencies may also need `LD_LIBRARY_PATH`
or a linker rpath-link. Pass `--gpu` to the validation runner to require GPU
placement. The compiler's own `--backend c|llvm` selects host code generation.

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

### Export an Aether circuit to Quake

`quantum_quake::quantum_export_quake(circuit,max_bytes,error,error_capacity)`
returns `Result<QuakeText,i32>`. It sends the validated ten-gate circuit through
the same CUDA-Q kernel builder used by state simulation and asks the SDK for
its Quake MLIR module. Qubit indices and bound f64 angles are preserved. Export
accepts 1–62 qubits and builds IR without executing the kernel or allocating an
amplitude vector. This is Aether-to-Quake export of a constructed circuit.
Importing Quake, CUDA-Q C++, or Python is outside this API.

On success, `.data` points to NUL-terminated UTF-8 and `.length` counts the
bytes before NUL. The text remains readable after freeing the circuit. Release
the owner with `quantum_quake_free(&mut text as *QuakeText)`. Copies alias the
same allocation. The byte budget includes the trailing NUL and covers the
returned text allocation. SDK builder and serialization workspace are additional.
Error codes 1, 2, 3 and 100 mean invalid input or ABI, output allocation failure,
insufficient output budget and SDK failure, respectively. The SDK generates the
kernel symbol, so independent exports can differ in spelling.

Rebuild the optional bridge to obtain the two new export/free symbols. Existing
state-vector applications can continue to use their version-1 bridge.

```sh
/path/to/aethc build examples/quantum/export_quake.aeth --sysroot . \
  --link-object /external/work/libaether_cudaq.dylib -o /external/work/export-quake
/external/work/export-quake > /external/work/circuit.mlir
python3 tests/cudaq_runtime.py --compiler /path/to/aethc \
  --bridge /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-test \
  --quake-python /path/to/python-with-cudaq
```

The verifier loads the exported module using CUDA-Q's MLIR bindings and checks
the register width, gate sequence, signed rotation angles and the distinct
control/target operands. The boundary tests include a 62-qubit IR export, small
budgets, invalid gates and release of the Aether-owned result.

`stdlib/cuda.aeth` separately exposes the CUDA runtime and cuBLAS C ABI with
fallible owned buffer/stream/handle helpers. Raw GPU pointers and asynchronous
lifetimes remain under caller control. Differentiation in the circuit layer
uses parameter-shift evaluations of the supported rotations.

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

## Explicit host-state budgets

The initial portable workload target is 16 qubits on devices with 8 GiB unified
memory; 16 GiB is an expansion configuration. Physical RAM is not an allocation
budget: the OS, application, runtime/JIT and device workspace need headroom.

`quantum_pool_new(byte_limit, max_qubits, max_live_states)` creates an explicitly
owned admission pool. For example `quantum_pool_new(2097152,16,2)` admits at most
two complex128 16-qubit host states, with 2 MiB total requested amplitude bytes.
Use `quantum_simulate_in_pool` or `quantum_cudaq_in_pool` to get a `QuantumLease`;
its `.state` is borrowed. Release through `quantum_lease_free`, never by calling
`quantum_state_free` separately. Allocation failure rolls back all allocations
and accounting. Freeing a lease returns its byte and state slots.

`quantum_pool_close` consumes the sole pool owner and refuses new allocations.
Existing leases retain the pool and remain readable/releasable. With no leases,
close frees the pool immediately; its raw pointer must then not be used. Pointer
copies are borrowed aliases, not extra owners. Calls on one pool must be
serialized; this is not an implementation of a concurrent GPU scheduler.
The original unpooled API and QuantumState layout remain unchanged.

The budget covers requested **host amplitude bytes only**. Metadata, gate lists,
allocator overhead, SDK/JIT workspace and internal host/device copies are outside
it. It is not a process RSS limit or GPU memory guarantee. `quantum_state_bytes`
provides checked complex128 planning independently of the existing 24-qubit
allocation ceiling; requests whose byte count overflows u64 are rejected.

| Pure state qubits | complex64 (Aether compute) | complex128 (Aether reference and CUDA-Q adapter) |
| --- | --- | --- |
| 16 | 512 KiB | 1 MiB |
| 20 | 8 MiB | 16 MiB |
| 24 | 128 MiB | 256 MiB |
| 28 | 2 GiB | 4 GiB |

These are byte counts for a single `2^n` pure state vector. A full 16-qubit
complex128 density matrix would require `4^16 * 16` bytes, or 64 GiB. The
current simulators represent pure states. Aether's Metal and Vulkan paths use
two f32 lanes per amplitude and compare results against a complex128 host
reference. Vulkan shader fp64 support varies by device. [VkPhysicalDeviceFeatures](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceFeatures.html)

New validation builds a 16-qubit GHZ state, checks all 65,536 amplitudes, exact
budget boundaries, concurrent live-state admission, close-before-release, and
allocation failure rollback. CPU reference and native CUDA-Q qpp versions pass
C/LLVM O0/O2. On this 16 GiB Mac, single native O2 measurements report total RSS
3,866,624 B for the reference fixture and 60,325,888 B for the SDK comparison;
both use a 2 MiB managed host-state peak. These are not measurements from an
8 GiB phone. The added pooled path was validated on Mac CPU; earlier NVIDIA
validation covers the unpooled API, eight mixed circuits and optimization.

```sh
python3 tests/quantum_values.py --compiler /path/to/aethc \
  --work-dir /external/work/quantum
# Includes both the original mixed-circuit workload and the 16-qubit pool test:
python3 tests/cudaq_runtime.py --compiler /path/to/aethc \
  --bridge /external/work/libaether_cudaq.dylib --work-dir /external/work/cudaq-test
```

## Shared compute queue

`quantum_compute(queue,circuit)` supports the ten Circuit gates on the F32 CPU, Metal and Vulkan GEMM providers. Amplitudes use two f32 lanes; observables accumulate in f64. Aether packs disjoint amplitude pairs and encodes the complex gate into a real 4×4 operator. It waits for the device event before scattering the completed amplitudes. Controlled and swap gates select only active pairs. Batches contain at most 32,768 pairs. Admission charges host state, packing and native buffers to the same queue; driver metadata and SDK-internal workspace remain outside the specified budget. Use `quantum_compute_free` to release the result. Scope is a single serialized queue with explicit provider selection.

`quantum_compute_gradient` uses parameter shift with separate rotations, including when a large finite angle would round away an added shift. `quantum_compute_cudaq` accepts an explicit SDK placement requirement, returns the same readable state, and propagates SDK failures and placement mismatches. Its host f64 return and f32 conversion storage both count against the queue. SDK verification requires the optional CUDA-Q bridge.

`tests/compute_runtime.py` runs the 16q all-gate workload, checks every amplitude against the f64 reference, all three Pauli axes, analytic gradient, a classical optimizer update, rollback and zero remaining bytes. Use `--metal` or `--device vulkan --provider /absolute/path/libaether_vulkan.so` to require a real GPU. `tests/cudaq_runtime.py` checks the same circuit through the SDK and verifies the reported state placement.

### Reading a subset of qubits

`quantum_marginal(state, selected, count, output, length)` writes the exact
computational-basis distribution of the selected qubits. Supply a disjoint
output array of exactly `2^count` f64 elements. Position `j` in `selected`
is bit `j` of the output bin. An empty selection produces the total squared
norm in one bin. Duplicate or out-of-range indices and incorrect lengths
return an error before writing. The computation preserves the state and uses
fixed scratch space independent of state size.

`quantum_compute_marginal` accepts a state produced on a compute queue. It
reads the synchronized f32 amplitudes already held by the simulator and
accumulates probabilities on the host in f64. CUDA-Q results exported through
the fp64 bridge use `quantum_marginal`. Two Bell states can have identical
one-qubit marginals and disjoint two-qubit distributions. The reference tests
exercise that distinction and the 16-qubit workload checks the specified bit
order against analytical probabilities.
