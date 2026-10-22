"""Shared notice manifest validation; no network or Cargo invocation."""
import hashlib
import json
from pathlib import Path
import platform

COMPONENT = Path(__file__).resolve().parents[1]
GENERATED_FILES = ("THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_NOTICES.html")


def native_target():
    machine = platform.machine().lower()
    architecture = {"arm64": "aarch64", "amd64": "x86_64"}.get(machine, machine)
    suffix = {"Darwin": "apple-darwin", "Linux": "unknown-linux-gnu",
              "Windows": "pc-windows-msvc"}.get(platform.system())
    if suffix is None or architecture not in ("aarch64", "x86_64"):
        raise ValueError("Specify --target for this platform")
    return f"{architecture}-{suffix}"


def sha256(contents):
    return hashlib.sha256(contents).hexdigest()


def output_directory(component, target):
    return component / "target/third-party-notices" / target


def configuration(component, target, default_features=True, features=()):
    paths = [
        "Cargo.lock", "Cargo.toml", "app/Cargo.toml", "about.toml",
        "crates/maple-agent/src/store/keys/SOURCES.md",
        "crates/maple-agent/src/store/keys/eff-large-wordlist.txt",
        "app/assets/dict/LICENSE", "dependencies/sqlcipher/provenance.json",
        "dependencies/licenses/CUA-LICENSE.md",
        "scripts/generate-third-party-notices.py", "scripts/third_party_notices.py",
    ]
    paths.extend(str(path.relative_to(component)) for path in (component / "crates").glob("*/Cargo.toml"))
    # These local dependencies are present in monorepo and pure Nix builds.
    paths.extend(name for name in ("../../sdk/rust/Cargo.toml", "../../proxy/Cargo.toml")
                 if (component / name).is_file())
    inputs = {}
    for name in sorted(set(paths)):
        path = component / name
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Missing notice input: {name}")
        inputs[name] = sha256(path.read_bytes())
    return {"schema": 1, "package": "maple-agent-app", "target": target,
            "default_features": default_features, "features": sorted(set(features)), "inputs": inputs}


def validate(component, directory, target, default_features=True, features=()):
    manifest_path = directory / "manifest.json"
    if not manifest_path.is_file() or manifest_path.is_symlink():
        raise ValueError("Generate third-party notices before packaging")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    expected = configuration(component, target, default_features, features)
    outputs = manifest.pop("outputs", {})
    if manifest != expected or set(outputs) != set(GENERATED_FILES):
        raise ValueError("Third-party notices do not match the current target, features or source inputs")
    for name in GENERATED_FILES:
        path = directory / name
        if not path.is_file() or path.is_symlink() or sha256(path.read_bytes()) != outputs[name]:
            raise ValueError(f"Missing or modified third-party notices: {name}")
