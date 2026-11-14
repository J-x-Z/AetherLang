#!/usr/bin/env python3
"""Build scripts must export paths usable after subprocesses change directory."""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='aether-build-paths-') as directory:
    work = Path(directory).resolve()
    tools = work / 'bin'
    tools.mkdir()
    stubs = {
        'cargo': '''#!/bin/sh
if [ "$1" = --version ]; then echo cargo-probe; exit 0; fi
printf '%s\\n%s\\n' "$CARGO_TARGET_DIR" "$TMPDIR" > "$AETHER_PATH_PROBE"
exit 42
''',
        'rustc': '#!/bin/sh\necho rustc-probe\n',
        'rustup': '#!/bin/sh\nexit 1\n',
    }
    for name, source in stubs.items():
        tool = tools / name
        tool.write_text(source)
        tool.chmod(0o755)
    for script in ['build', 'test']:
        for custom_tmp in [False, True]:
            output = work / 'environment.txt'
            target = work / f'{script}-{custom_tmp}'
            tmp = work / 'custom-tmp' if custom_tmp else target / 'tmp'
            env = dict(os.environ, PATH=f'{tools}:/usr/bin:/bin',
                       CARGO_TARGET_DIR=os.path.relpath(target, root),
                       AETHER_PATH_PROBE=str(output))
            env.pop('TMPDIR', None)
            if custom_tmp:
                env['TMPDIR'] = os.path.relpath(tmp, root)
            run = subprocess.run(['sh', str(root / 'scripts' / f'{script}.sh'), 'c'],
                                 cwd=root, env=env, capture_output=True, text=True, timeout=10)
            assert run.returncode == 42, (run.returncode, run.stdout, run.stderr)
            assert output.read_text().splitlines() == [str(target), str(tmp)]
            # A child working outside the repository can use both exported paths.
            subprocess.run(['sh', '-c', 'test -d "$1" && touch "$2/child-file"',
                            'probe', str(target), str(tmp)], cwd=work, check=True)
print('PASS: build/test preserve relative output paths across child working directories')
