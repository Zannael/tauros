#!/usr/bin/env python3
"""Prepare pinned local whisper.cpp and model under TAUROS_DATA (stdlib only)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def run(args):
    subprocess.run([str(arg) for arg in args], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fetch-only', action='store_true', help='Fetch pinned sources/model for review without running external build files')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'toolchain/voice.json').read_text())
    data = Path(os.environ.get('TAUROS_DATA', '~/.local/share/tauros')).expanduser().resolve()
    if data == ROOT or ROOT in data.parents:
        raise SystemExit('TAUROS_DATA must be outside the repository')
    engine, model = manifest['engine'], manifest['model']
    source = data / 'dependencies' / ('whisper.cpp-v' + engine['version'])
    source.parent.mkdir(parents=True, exist_ok=True)
    if not source.exists():
        run(['git', '-c', 'core.hooksPath=/dev/null', 'clone', '--no-checkout', '--depth', '1', '--branch', 'v' + engine['version'], engine['url'], source])
        run(['git', '-C', source, '-c', 'core.hooksPath=/dev/null', 'checkout', '--detach', engine['commit']])
    actual = subprocess.check_output(['git', '-C', source, 'rev-parse', 'HEAD'], text=True).strip()
    if actual != engine['commit']:
        raise SystemExit('Refusing an unexpected whisper.cpp revision')
    run(['git', '-C', source, 'remote', 'set-url', '--push', 'origin', 'DISABLED-by-tauros'])
    if subprocess.check_output(['git', '-C', source, 'status', '--porcelain', '--untracked-files=no'], text=True).strip():
        raise SystemExit('Refusing a modified whisper.cpp source tree')
    models = data / 'models' / 'whisper'
    models.mkdir(parents=True, exist_ok=True)
    dest = models / model['name']
    def valid(path):
        if not path.is_file() or path.stat().st_size != model['size']:
            return False
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(chunk)
        return digest.hexdigest() == model['sha256']
    if not valid(dest):
        partial = dest.with_suffix('.download')
        try:
            with urllib.request.urlopen(model['url'], timeout=60) as response, partial.open('wb') as stream:
                shutil.copyfileobj(response, stream)
            if not valid(partial):
                raise SystemExit('Whisper model checksum/size mismatch')
            partial.replace(dest)
        finally:
            partial.unlink(missing_ok=True)
    print('Pinned source and checksum-verified model ready.', flush=True)
    if args.fetch_only:
        print('Review external CMake files before running this script without --fetch-only.')
        return
    run(['cmake', '-S', source, '-B', source / 'build', '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF', '-DGGML_NATIVE=OFF', '-DGGML_OPENMP=OFF', '-DWHISPER_BUILD_TESTS=OFF', '-DWHISPER_BUILD_EXAMPLES=ON', '-DWHISPER_BUILD_SERVER=OFF', '-DWHISPER_CURL=OFF', '-DGGML_CPU_KLEIDIAI=OFF'])
    run(['cmake', '--build', source / 'build', '--target', 'whisper-cli', '-j', '2'])
    print('Local voice ready. Restart Tauros and press Record in the chat.')


if __name__ == '__main__':
    main()
