// CUDA-Q SDK adapter only: Aether owns circuits, validation, optimizers and
// the reference simulator. No C++ container or exception crosses this ABI.
#include "cudaq/builder/kernel_builder.h"
#include "cudaq/algorithms/get_state.h"
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <exception>
#include <mutex>
#include <vector>
struct AetherQuantumGate { std::int32_t kind; std::uint32_t target, control; double angle; };
struct AetherComplex64 { double re, im; };
static_assert(sizeof(AetherQuantumGate)==24 && offsetof(AetherQuantumGate,angle)==16);
static_assert(sizeof(AetherComplex64)==16);
static std::mutex sdk_mutex;
static int failure(int code,const char *message,char *error,std::uint64_t capacity) noexcept {
  if(error && capacity) { auto n=std::strlen(message); if(n>=capacity)n=capacity-1; std::memcpy(error,message,n);error[n]=0; }
  return code;
}
extern "C" std::uint32_t aether_cudaq_abi_version() noexcept { return 1; }

// Both execution and IR export consume precisely this validated gate stream.
template <typename Consumer>
static int with_kernel(std::uint32_t qubits, const AetherQuantumGate *gates,
                       std::uint64_t count, char *error, std::uint64_t error_capacity,
                       Consumer &&consume) noexcept {
  if (!qubits || qubits > 62 || (count && !gates))
    return failure(1, "invalid circuit", error, error_capacity);
  for(std::uint64_t i=0;i<count;++i) {
    const auto &g=gates[i];
    if(g.kind<1 || g.kind>10 || g.target>=qubits || !std::isfinite(g.angle) ||
       (g.kind>=8 && (g.control>=qubits || g.control==g.target)))
      return failure(1,"invalid gate",error,error_capacity);
  }
  try {
    std::lock_guard<std::mutex> lock(sdk_mutex);
    auto kernel=cudaq::make_kernel(); auto q=kernel.qalloc(qubits);
    for(std::uint64_t i=0;i<count;++i) {
      const auto &g=gates[i];auto target=q[g.target];
      switch(g.kind) {
        case 1:kernel.h(target);break;case 2:kernel.x(target);break;
        case 3:kernel.y(target);break;case 4:kernel.z(target);break;
        case 5:kernel.rx(g.angle,target);break;case 6:kernel.ry(g.angle,target);break;
        case 7:kernel.rz(g.angle,target);break;
        case 8:kernel.x<cudaq::ctrl>(q[g.control],target);break;
        case 9:kernel.z<cudaq::ctrl>(q[g.control],target);break;
        case 10:kernel.swap(q[g.control],target);break;
      }
    }
    return consume(kernel);
  } catch(const std::exception &e) {return failure(100,e.what(),error,error_capacity);}
  catch(...) {return failure(100,"unknown CUDA-Q exception",error,error_capacity);}
}

extern "C" std::int32_t aether_cudaq_state(std::uint32_t qubits,
    const AetherQuantumGate *gates,std::uint64_t count,AetherComplex64 *output,
    std::uint64_t capacity,char *error,std::uint64_t error_capacity,std::int32_t *on_gpu) noexcept {
  if(error && error_capacity)error[0]=0;
  if(on_gpu)*on_gpu=0;
  if(!qubits || qubits>24 || !output || capacity!=(std::uint64_t{1}<<qubits))
    return failure(1,"invalid circuit or state buffer",error,error_capacity);
  return with_kernel(qubits, gates, count, error, error_capacity, [&](auto &kernel) {
    auto state=cudaq::get_state(kernel);
    if(state.get_precision()!=cudaq::SimulationState::precision::fp64)
      return failure(100,"CUDA-Q backend must use fp64 state-vector precision",error,error_capacity);
    std::vector<std::complex<double>> host(capacity);
    if(state.is_on_gpu())state.to_host(host.data(),capacity);
    else {
      // CUDA-Q operator[] uses display (big-endian) indices. Tensor storage
      // and to_host use logical little-endian wires, matching Aether's ABI.
      const auto tensor=state.get_tensor();
      if(tensor.extents.size()!=1 || tensor.extents[0]!=capacity)
        return failure(100,"CUDA-Q backend did not return a state vector",error,error_capacity);
      const auto *data=static_cast<const std::complex<double> *>(tensor.data);
      std::copy(data,data+capacity,host.begin());
    }
    for(std::uint64_t i=0;i<capacity;++i)output[i]={host[i].real(),host[i].imag()};
    if(on_gpu)*on_gpu=state.is_on_gpu()?1:0;
    return 0;
  });
}

extern "C" std::int32_t aether_cudaq_quake(std::uint32_t qubits,
    const AetherQuantumGate *gates, std::uint64_t count, std::uint64_t max_bytes,
    char **output, std::uint64_t *length, char *error, std::uint64_t error_capacity) noexcept {
  if (error && error_capacity) error[0] = 0;
  if (output) *output = nullptr;
  if (length) *length = 0;
  if (!output || !length)
    return failure(1, "missing Quake output fields", error, error_capacity);
  return with_kernel(qubits, gates, count, error, error_capacity, [&](auto &kernel) {
    const auto text = kernel.to_quake();
    if (text.size() >= max_bytes)
      return failure(3, "Quake text exceeds byte budget including NUL", error, error_capacity);
    auto *copy = static_cast<char *>(std::malloc(text.size() + 1));
    if (!copy) return failure(2, "cannot allocate Quake text", error, error_capacity);
    std::memcpy(copy, text.c_str(), text.size() + 1);
    *output = copy;
    *length = text.size();
    return 0;
  });
}

extern "C" void aether_cudaq_quake_free(char *text) noexcept { std::free(text); }
