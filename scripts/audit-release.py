"""Fail closed on unexpected bundle contents or common credential/personal-path leaks.
Prints filenames and rule names only, never matched secret bytes. This is a release
hygiene check, not a claim that pattern matching can detect every possible secret.
"""
import argparse
import re
import subprocess
from pathlib import Path

RULES = {
    'OpenRouter credential': rb'sk-or-v1-[A-Za-z0-9_-]{20,}',
    'GitHub credential': rb'(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})',
    'private signing key': rb'-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----',
}
BUNDLE_FILES = {
    'Contents/Info.plist', 'Contents/MacOS/DramaFactory',
    'Contents/Resources/DramaFactory.icns', 'Contents/Resources/Assets.car',
    'Contents/_CodeSignature/CodeResources',
}

def scan(path, personal=False):
    data = path.read_bytes()
    for name, pattern in RULES.items():
        if re.search(pattern, data):
            raise SystemExit(f'Release audit failed: {name} in {path.name}')
    if personal and re.search(rb'/(?:Users|home)/[A-Za-z0-9_.-]+/', data):
        raise SystemExit(f'Release audit failed: home-folder path in {path.name}')

parser = argparse.ArgumentParser()
parser.add_argument('--bundle', type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[1]
for name in subprocess.check_output(['git', 'ls-files', '-z'], cwd=repo).decode().split('\0'):
    if not name:
        continue
    path = repo/name
    if path.is_file():
        if path.suffix in {'.pem', '.p12', '.pfx', '.key', '.sqlite', '.db'} or path.name.startswith('.env'):
            raise SystemExit(f'Release audit failed: private data file tracked: {name}')
        scan(path)
if args.bundle:
    seen = set()
    for path in args.bundle.rglob('*'):
        if path.is_symlink():
            raise SystemExit('Release audit failed: unexpected bundle symlink')
        if path.is_file():
            relative = path.relative_to(args.bundle).as_posix()
            if relative not in BUNDLE_FILES:
                raise SystemExit(f'Release audit failed: unexpected bundled file {relative}')
            seen.add(relative)
            scan(path, personal=True)
    if seen != BUNDLE_FILES:
        raise SystemExit('Release audit failed: incomplete app bundle')
print('Release audit passed: tracked-file credential scan and requested bundle allowlist/path checks.')
