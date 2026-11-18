"""Choose a matching reader for emitted textual LLVM IR in sanitizer tests."""
import os
import subprocess
import sys


def emitted_source_compiler(backend, host_cc):
    ir_cc = os.getenv('AETHER_IR_CC')
    if backend != 'llvm' or not ir_cc:
        return [host_cc]
    if sys.platform == 'darwin':
        # Homebrew's default config can point to an absent CommandLineTools SDK.
        sdk = os.getenv('SDKROOT') or subprocess.check_output(
            ['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip()
        return [ir_cc, '--no-default-config', '-isysroot', sdk]
    return [ir_cc]
