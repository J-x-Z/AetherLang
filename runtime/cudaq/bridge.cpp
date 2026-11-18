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
struct AetherTemplateGate { AetherQuantumGate gate; std::int64_t slot; };
static_assert(sizeof(AetherTemplateGate)==32 && offsetof(AetherTemplateGate,slot)==24);
static const AetherQuantumGate &gate_of(const AetherQuantumGate &g) { return g; }
static const AetherQuantumGate &gate_of(const AetherTemplateGate &g) { return g.gate; }
static std::int64_t slot_of(const AetherQuantumGate &) { return -1; }
static std::int64_t slot_of(const AetherTemplateGate &g) { return g.slot; }
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
template <typename Record, typename Consumer>
static int with_kernel(std::uint32_t qubits, const Record *gates,
                       std::uint64_t count, std::uint32_t slots, char *error, std::uint64_t error_capacity,
                       Consumer &&consume) noexcept {
  if (!qubits || qubits > 62 || (count && !gates))
    return failure(1, "invalid circuit", error, error_capacity);
  for(std::uint64_t i=0;i<count;++i) {
    const auto &g=gate_of(gates[i]);
    const auto slot=slot_of(gates[i]);
    if (slot < -1 || (slot >= 0 && (slot >= slots || g.kind < 5 || g.kind > 7)))
      return failure(1,"invalid rotation parameter slot",error,error_capacity);
    if(g.kind<1 || g.kind>10 || g.target>=qubits || !std::isfinite(g.angle) ||
       (g.kind>=8 && (g.control>=qubits || g.control==g.target)))
      return failure(1,"invalid gate",error,error_capacity);
  }
  try {
    std::lock_guard<std::mutex> lock(sdk_mutex);
    // The public builder takes its function signature at construction. Match the
    // Aether slot count exactly, including declared but unused parameters.
    double scalar = 0.0;
    std::vector<cudaq::detail::KernelBuilderType> types(slots, cudaq::detail::convertArgumentTypeToMLIR(scalar));
    cudaq::kernel_builder<> kernel(types);
    auto q=kernel.qalloc(qubits);
    for(std::uint64_t i=0;i<count;++i) {
      const auto &g=gate_of(gates[i]);auto target=q[g.target];
      switch(g.kind) {
        case 1:kernel.h(target);break;case 2:kernel.x(target);break;
        case 3:kernel.y(target);break;case 4:kernel.z(target);break;
        case 5:case 6:case 7: {
          auto rotate=[&](auto &angle) {
            if(g.kind==5) kernel.rx(angle,target);
            else if(g.kind==6) kernel.ry(angle,target);
            else kernel.rz(angle,target);
          };
          const auto slot=slot_of(gates[i]);
          if(slot<0) rotate(g.angle);
          else rotate(kernel.getArguments()[static_cast<std::size_t>(slot)]);
          break;
        }
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
  return with_kernel(qubits, gates, count, 0, error, error_capacity, [&](auto &kernel) {
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

template <typename Record>
static std::int32_t export_kernel(std::uint32_t qubits,
    const Record *gates, std::uint64_t count, std::uint32_t slots, std::uint64_t max_bytes,
    char **output, std::uint64_t *length, char *error, std::uint64_t error_capacity) noexcept {
  if (error && error_capacity) error[0] = 0;
  if (output) *output = nullptr;
  if (length) *length = 0;
  if (!output || !length)
    return failure(1, "missing Quake output fields", error, error_capacity);
  // Each serialized f64 argument needs at least three characters. Reject an
  // impossible text budget before allocating SDK descriptors for the signature.
  if (slots && static_cast<std::uint64_t>(slots)*3 >= max_bytes)
    return failure(3, "Quake signature exceeds byte budget", error, error_capacity);
  return with_kernel(qubits, gates, count, slots, error, error_capacity, [&](auto &kernel) {
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

extern "C" std::int32_t aether_cudaq_quake(std::uint32_t qubits,
    const AetherQuantumGate *gates, std::uint64_t count, std::uint64_t max_bytes,
    char **output, std::uint64_t *length, char *error, std::uint64_t error_capacity) noexcept {
  return export_kernel(qubits,gates,count,0,max_bytes,output,length,error,error_capacity);
}

extern "C" std::int32_t aether_cudaq_template_quake(std::uint32_t qubits,
    const AetherTemplateGate *gates, std::uint64_t count, std::uint32_t slots,
    std::uint64_t max_bytes, char **output, std::uint64_t *length,
    char *error, std::uint64_t error_capacity) noexcept {
  return export_kernel(qubits,gates,count,slots,max_bytes,output,length,error,error_capacity);
}

extern "C" void aether_cudaq_quake_free(char *text) noexcept { std::free(text); }
