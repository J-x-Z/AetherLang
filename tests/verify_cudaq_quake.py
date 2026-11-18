"""Parse the export example with the CUDA-Q Python SDK and verify gate operands."""
from pathlib import Path
import sys

from cudaq.mlir import ir

with ir.Context():
    module = ir.Module.parse(Path(sys.argv[1]).read_text())
    assert module.operation.verify()
    functions = list(module.body.operations)
    assert len(functions) == 1
    function = functions[0].operation
    assert function.name == 'func.func' and 'cudaq-entrypoint' in function.attributes
    gates = []
    registers = []
    for item in function.regions[0].blocks[0].operations:
        op = item.operation
        if op.name == 'quake.alloca':
            registers.append(op.result)
            assert str(op.result.type) == '!quake.veq<3>'
        elif op.name in ('quake.extract_ref', 'arith.constant', 'func.return'):
            continue
        else:
            arguments = []
            for operand in op.operands:
                owner = operand.owner.operation
                if owner.name == 'quake.extract_ref':
                    assert len(registers) == 1 and owner.operands[0] == registers[0]
                    arguments.append(ir.IntegerAttr(owner.attributes['rawIndex']).value)
                else:
                    assert owner.name == 'arith.constant' and str(operand.type) == 'f64'
                    arguments.append(ir.FloatAttr(owner.attributes['value']).value)
            segments = tuple(ir.DenseI32ArrayAttr(op.attributes['operandSegmentSizes']))
            gates.append((op.name, segments, arguments))
    # Segment lengths denote angle, control, and target operands, respectively.
    expected = [('quake.h', (0, 0, 1), [2]), ('quake.x', (0, 0, 1), [1]),
                ('quake.y', (0, 0, 1), [0]), ('quake.z', (0, 0, 1), [2]),
                ('quake.rx', (1, 0, 1), [0.625, 2]), ('quake.ry', (1, 0, 1), [-0.375, 1]),
                ('quake.rz', (1, 0, 1), [0.125, 0]), ('quake.x', (0, 1, 1), [2, 0]),
                ('quake.z', (0, 1, 1), [0, 2]), ('quake.swap', (0, 0, 2), [0, 1])]
    assert gates == expected, gates
print('CUDA-Q verified the module, register width, ten gates, angles and control/target order')
