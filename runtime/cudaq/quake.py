"""Load Aether's exported f64-parameter Quake kernels in the CUDA-Q 0.16 SDK."""
import math

from cudaq import cudaq_runtime
from cudaq.kernel.utils import getMLIRContext
from cudaq.mlir import ir


class QuakeKernel:
    """One parsed module and its SDK compilation cache for repeated state evaluations."""

    def __init__(self, source):
        ctx = getMLIRContext()
        self.module = ir.Module.parse(source, context=ctx)
        self.module.operation.verify()
        entries = [op for op in self.module.body.operations
                   if op.operation.name == 'func.func' and 'cudaq-entrypoint' in op.attributes]
        if len(entries) != 1:
            raise ValueError('expected one CUDA-Q entry point')
        entry = entries[0]
        signature = ir.FunctionType(ir.TypeAttr(entry.attributes['function_type']).value)
        if len(signature.results) or any(not ir.F64Type.isinstance(t) for t in signature.inputs):
            raise ValueError('expected a void kernel with scalar f64 parameters')
        self.arity = len(signature.inputs)
        symbol = ir.StringAttr(entry.attributes['sym_name']).value
        prefix = '__nvqpp__mlirgen__'
        if not symbol.startswith(prefix):
            raise ValueError('expected a CUDA-Q builder kernel symbol')
        self.name = symbol[len(prefix):]
        # The C++ builder exports device IR. The Python SDK launcher also needs
        # a host entry in its name map to generate the parameter-marshalling thunk.
        # Attach linkage metadata once, leaving all instructions and arguments intact.
        if 'quake.mangled_name_map' not in self.module.operation.attributes:
            host_name = ir.StringAttr.get(self.name + '.PyKernelFakeEntryPoint', context=ctx)
            self.module.operation.attributes['quake.mangled_name_map'] = ir.DictAttr.get(
                {symbol: host_name}, context=ctx)
        self._cache = cudaq_runtime.CompiledModuleCache()

    def get_state(self, *angles):
        if len(angles) != self.arity:
            raise ValueError(f'expected {self.arity} angles, got {len(angles)}')
        values = [float(angle) for angle in angles]
        if not all(map(math.isfinite, values)):
            raise ValueError('angles must be finite')
        # CUDA-Q's Python get_state also passes a fresh lowering clone to the
        # launcher while retaining the same module compilation cache across calls.
        return cudaq_runtime.get_state_impl(self.name, cudaq_runtime.cloneModule(self.module),
                                            self._cache, *values)
