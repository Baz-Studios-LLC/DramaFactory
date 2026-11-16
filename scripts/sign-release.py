"""Prepare local signed release assets. Does not upload or publish anything.
Requires: uv pip install cryptography
Usage: .venv/bin/python scripts/sign-release.py --version v002 --binary target/release/dramafactory --output target/release-assets
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path
from cryptography.hazmat.primitives.serialization import load_pem_private_key

parser = argparse.ArgumentParser()
parser.add_argument("--version", required=True)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--key", type=Path, default=Path.home()/"Library/Application Support/studio.DramaFactory.DramaFactory/signing/update-private.pem")
args = parser.parse_args()
if not re.fullmatch(r"v[0-9]{3,}", args.version) or int(args.version[1:]) < 1:
    parser.error("Use a whole-number version such as v002")
if subprocess.check_output([str(args.binary.absolute()), "--version"], text=True).strip() != args.version:
    parser.error("Executable's embedded version does not match release label")
key = load_pem_private_key(args.key.read_bytes(), password=None)
data = args.binary.read_bytes()
manifest = dict(version=args.version, target="aarch64-apple-darwin",
                binary=f"DramaFactory-{args.version}-aarch64-apple-darwin",
                sha256=hashlib.sha256(data).hexdigest(), size=len(data))
signed = "".join(str(manifest[k])+"\n" for k in ["version", "target", "binary", "sha256", "size"]).encode()
manifest["signature"] = key.sign(signed).hex()
args.output.mkdir(parents=True, exist_ok=True)
shutil.copy2(args.binary, args.output/manifest["binary"])
(args.output/"update-aarch64-apple-darwin.json").write_text(json.dumps(manifest, indent=2)+"\n")
print(f"Signed {args.version} assets prepared locally at {args.output}. Nothing published.")
