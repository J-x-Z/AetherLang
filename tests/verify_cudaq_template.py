"""SDK signature, gate use and repeated JIT execution of one Aether Quake export."""
import cmath
import math
from pathlib import Path
import sys

import cudaq
from cudaq.mlir import ir

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'runtime/cudaq'))
from quake import QuakeKernel

gpu = '--gpu' in sys.argv[2:]
if gpu:
    cudaq.set_target('nvidia', option='fp64')
else:
    cudaq.set_target('qpp-cpu')
artifact = QuakeKernel(Path(sys.argv[1]).read_text(encoding='utf-8'))
assert artifact.arity == 3
block = next(iter(artifact.module.body.operations)).regions[0].blocks[0]
rotations = [op.operation for op in block.operations if op.operation.name in ('quake.rx', 'quake.ry', 'quake.rz')]
assert [op.name for op in rotations] == ['quake.ry','quake.ry','quake.rx','quake.rx','quake.rz']
assert rotations[0].operands[0] == rotations[1].operands[0] == block.arguments[0]
assert rotations[3].operands[0] == rotations[4].operands[0] == block.arguments[1]
assert ir.FloatAttr(rotations[2].operands[0].owner.operation.attributes['value']).value == 0.5

for bad in [(0., 1.), (0., 1., 2., 3.), (0., float('nan'), 0.), (0., 0., float('inf'))]:
    try:
        artifact.get_state(*bad)
    except ValueError:
        continue
    raise AssertionError(f'accepted invalid angles {bad}')

worst = 0.
for theta, phi, unused in [(0.,0.,0.), (.32,.4,-1.), (-.93,1.13,7.), (.32,.4,868.), (math.pi/2,-.9,0.)]:
    state = artifact.get_state(theta, phi, unused)
    assert state.num_qubits() == 3 and state.is_on_gpu() == gpu
    expected = [0j]*8
    half_x = (.5+phi)/2
    for high, weight in enumerate((math.cos(theta), math.sin(theta))):
        phase = cmath.exp(1j*(high-.5)*phi)
        for middle, component in enumerate((math.cos(half_x), -1j*math.sin(half_x))):
            expected[5*high + 2*middle] = weight*phase*component
    # This CX(2 -> 0) circuit makes bits 0 and 2 equal, independent of
    # the SDK's display/tensor indexing convention.
    errors = [abs(state[i] - amp) for i, amp in enumerate(expected)]
    assert max(errors) < 2e-12, (theta, phi, errors)
    worst = max(worst, *errors)
print(f'One 3-f64 Quake module, 5 parameter sets, 40 complex amplitudes, '
      f'max_error={worst:.3g}, on_gpu={int(gpu)}')
