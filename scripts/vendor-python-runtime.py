"""Populate a copied Windows embedded runtime with hash-pinned plugin wheels.

Only build-time Python is needed; the resulting runtime needs neither pip nor
site initialization. Original dist-info metadata and license files are retained.
"""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--wheels", type=Path, default=ROOT / ".cache/plugin-wheels")
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    if not (runtime / "python.exe").is_file():
        raise ValueError("Expected a copied Windows embedded runtime")
    lock = json.loads((ROOT / "resources/python-dependencies.lock.json").read_text())
    args.wheels.mkdir(parents=True, exist_ok=True)
    destination = runtime / "Lib/site-packages"
    destination.mkdir(parents=True, exist_ok=True)
    for package in lock["packages"]:
        wheel = args.wheels / package["filename"]
        if not wheel.exists():
            temporary = wheel.with_suffix(".download")
            with urllib.request.urlopen(package["url"], timeout=60) as response:
                temporary.write_bytes(response.read(64 * 1024 * 1024 + 1))
            if hashlib.sha256(temporary.read_bytes()).hexdigest() != package["sha256"]:
                raise ValueError(f"Downloaded wheel integrity failed: {wheel.name}")
            temporary.replace(wheel)
        if hashlib.sha256(wheel.read_bytes()).hexdigest() != package["sha256"]:
            raise ValueError(f"Cached wheel integrity failed: {wheel.name}")
        with zipfile.ZipFile(wheel) as archive:
            for member in archive.infolist():
                target = (destination / member.filename).resolve()
                if not target.is_relative_to(destination.resolve()) or ".data" in Path(member.filename).parts:
                    raise ValueError(f"Unsupported wheel path: {member.filename}")
            archive.extractall(destination)
    pth = runtime / "python314._pth"
    lines = pth.read_text().splitlines()
    if "Lib/site-packages" not in lines:
        pth.write_text("\n".join([*lines, "Lib/site-packages", ""]), encoding="ascii")
    (runtime / "python-dependencies.lock.json").write_text(json.dumps(lock, indent=2) + "\n")
    print(json.dumps(dict(runtime=str(runtime), pinned_wheels=len(lock["packages"]))))


if __name__ == "__main__":
    main()
