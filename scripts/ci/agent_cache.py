#!/usr/bin/env python3
"""Fingerprint the actual Agent build environment before restoring Cargo targets.

Run inside the component's pinned Nix shell, or the selected Windows MSVC
developer environment. Cargo.lock deliberately belongs to rust-cache's exact
dependency suffix, not this compatibility boundary.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[2]
COMPONENT = ROOT / "apps/maple-agent"
SCHEMA = 1
CONFIG_FILES = (
    "apps/maple-agent/flake.nix",
    "apps/maple-agent/flake.lock",
    "apps/maple-agent/justfile",
    "apps/maple-agent/dependencies/sqlcipher/provenance.json",
    "apps/maple-agent/dependencies/sqlcipher/verify_sources.py",
    "apps/maple-agent/scripts/verify-sqlcipher-build.py",
    "apps/maple-agent/scripts/test-rusqlite-provider.py",
    "apps/maple-agent/scripts/build-release.sh",
    "scripts/ci/agent_cache.py",
)
ENV_KEYS = {
    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC",
    "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_INCREMENTAL",
    "RUSTC_BOOTSTRAP", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET",
    "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_RUSTC", "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "PKG_CONFIG", "CMAKE",
    "CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS",
    "NIX_CFLAGS_COMPILE", "NIX_LDFLAGS", "NIX_CC", "NIX_BINTOOLS",
    "NIX_ENFORCE_PURITY", "SDKROOT", "DEVELOPER_DIR",
    "MACOSX_DEPLOYMENT_TARGET", "MAPLE_RELEASE_PROFILE",
    "VITE_OPEN_SECRET_PCR_ENVIRONMENT", "OPENSSL_SRC_PERL",
    "INCLUDE", "LIB", "LIBPATH", "VCToolsInstallDir", "VCToolsVersion",
    "WindowsSdkDir", "WindowsSDKVersion", "WindowsSDKLibVersion",
    "VSCMD_ARG_TGT_ARCH", "VSCMD_ARG_HOST_ARCH",
}
ENV_PREFIXES = (
    "CARGO_PROFILE_", "CARGO_TARGET_", "CFLAGS_", "CXXFLAGS_", "CC_",
    "CXX_", "AR_", "OPENSSL_", "SQLCIPHER_", "SQLITE_", "LIBSQLITE3_",
    "PKG_CONFIG_", "CMAKE_", "NIX_CFLAGS_", "NIX_LDFLAGS_",
)
# These labels describe the actual commands retained by agent-ci.yml and the
# release helper. Relevant manifest tables and the command owners are hashed
# below, so edits to their features/profile settings cannot cross this boundary.
PROFILES = {
    "ci": ["workspace:all-targets", "workspace:test", "app:acp,proxy", "app:acp", "app:proxy"],
    "release": ["release:maple-agent-app:default-features"],
    "provider": ["test:bundled-sqlcipher,modern-full", "libsqlite3-sys:bundled-sqlcipher-openssl4"],
}


def digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command(arguments: list[str], *, allowed: tuple[int, ...] = (0,)) -> str:
    selected = subprocess.run(arguments, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False)
    output = "\n".join(part.strip() for part in (selected.stdout, selected.stderr) if part.strip())
    if selected.returncode not in allowed:
        raise ValueError(f"cannot identify build tool {arguments[0]} (exit {selected.returncode})")
    if not output:
        raise ValueError(f"build tool returned no identity: {arguments[0]}")
    return output


def executable(name: str) -> str:
    found = shutil.which(name)
    if not found:
        raise ValueError(f"required build tool is missing: {name}")
    return str(Path(found).resolve())


def compiler_identity(selection: str) -> dict[str, object]:
    arguments = shlex.split(selection)
    if not arguments:
        raise ValueError("native compiler selection is empty")
    path = executable(arguments[0])
    return {
        "command": [path, *arguments[1:]],
        "version": command([path, *arguments[1:], "--version"]),
        "target": command([path, *arguments[1:], "-dumpmachine"]),
    }


def native_build_tools(environment: dict[str, str]) -> dict[str, object]:
    perl = executable(environment.get("OPENSSL_SRC_PERL", "perl"))
    result = {"perl": {"path": perl, "version": command([perl, "-e", "print qq($^V\\n)"])}}
    # openssl-src selects NASM when present, otherwise builds without assembly.
    # Include absence explicitly so runner-image changes cannot cross that
    # native-configuration boundary using a compatible target-cache restore.
    for name in ("nasm", "cmake", "pkg-config"):
        selected = shutil.which(name)
        result[name] = (
            {"path": str(Path(selected).resolve()), "version": command([selected, "--version"])}
            if selected else None
        )
    return result


def native_identity(system: str, environment: dict[str, str]) -> dict[str, object]:
    if system == "Windows":
        # Python exposes uppercase keys on Windows; copied environment mappings
        # no longer retain os.environ's case-insensitive lookup behavior.
        canonical = {key.upper(): key for key in ENV_KEYS}
        environment = {canonical.get(key.upper(), key.upper()): value for key, value in environment.items()}
        required = ("VCToolsInstallDir", "WindowsSdkDir", "WindowsSDKVersion")
        if any(not environment.get(key) for key in required):
            raise ValueError("select the MSVC developer environment before fingerprinting Windows targets")
        compiler = Path(executable("cl.exe")).resolve()
        toolset = Path(environment["VCToolsInstallDir"]).resolve()
        if not compiler.is_relative_to(toolset):
            raise ValueError("cl.exe is outside the selected MSVC toolset")
        # cl's driver banner alone does not identify its compiler/linker DLLs.
        binaries = {}
        for name in ("cl.exe", "c1.dll", "c1xx.dll", "c2.dll", "link.exe"):
            candidate = compiler.parent / name
            if not candidate.is_file():
                raise ValueError(f"selected MSVC tool is missing: {name}")
            binaries[name] = file_digest(candidate)
        sdk = Path(environment["WindowsSdkDir"]).resolve()
        sdk_version = environment["WindowsSDKVersion"].rstrip("/\\")
        if not (sdk / "Include" / sdk_version).is_dir() or not (sdk / "Lib" / sdk_version).is_dir():
            raise ValueError("selected Windows SDK headers or libraries are missing")
        return {
            "compiler": str(compiler), "toolset": str(toolset),
            "compiler_version": command([str(compiler)], allowed=(0, 2)),
            "binaries": binaries, "sdk": str(sdk), "sdk_version": sdk_version,
        }
    result = {
        "cc": compiler_identity(environment.get("CC", "cc")),
        "cxx": compiler_identity(environment.get("CXX", "c++")),
    }
    if system == "Darwin":
        result["xcode"] = command(["/usr/bin/xcodebuild", "-version"])
        result["sdk_path"] = command(["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path"])
        result["sdk_version"] = command(["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-version"])
    return result


def dependency_feature_settings(value: dict, prefix: str = "") -> dict[str, object]:
    """Keep feature selection at workspace and target scopes, excluding versions."""
    settings = {}
    for key, child in value.items():
        if not isinstance(child, dict):
            continue
        name = f"{prefix}.{key}" if prefix else key
        if key in ("dependencies", "dev-dependencies", "build-dependencies"):
            for dependency, selection in child.items():
                if isinstance(selection, dict):
                    flags = {field: selection[field] for field in
                             ("features", "default-features", "optional", "workspace", "package")
                             if field in selection}
                    if flags:
                        settings[f"{name}.{dependency}"] = flags
        else:
            settings.update(dependency_feature_settings(child, name))
    return settings


def manifest_settings(root: Path) -> dict[str, object]:
    component = root / "apps/maple-agent"
    manifests = sorted({
        component / "Cargo.toml", component / "app/Cargo.toml",
        *component.glob("crates/*/Cargo.toml"),
        root / "sdk/rust/Cargo.toml", root / "proxy/Cargo.toml",
    })
    result = {}
    for path in manifests:
        manifest = tomllib.loads(path.read_text(encoding="utf-8"))
        package = manifest.get("package", {})
        workspace = manifest.get("workspace", {})
        # Package versions and dependency locks belong to the exact key. These
        # settings instead affect the ABI, enabled code, or build profile.
        result[str(path.relative_to(root))] = {
            "features": manifest.get("features", {}),
            "dependency-features": dependency_feature_settings(manifest),
            "profile": manifest.get("profile", {}),
            "resolver": manifest.get("resolver", workspace.get("resolver")),
            "edition": package.get("edition", workspace.get("package", {}).get("edition")),
            "rust-version": package.get("rust-version", workspace.get("package", {}).get("rust-version")),
        }
    return result


def cargo_configuration(directory: Path, root: Path, cargo_home: Path) -> dict[str, str]:
    """Hash config contents without exposing external paths or config secrets."""
    result = {}
    directory, root = directory.resolve(), root.resolve()
    for depth, ancestor in enumerate((directory, *directory.parents)):
        label = str(ancestor.relative_to(root)) if ancestor.is_relative_to(root) else f"ancestor-{depth}"
        for name in ("config", "config.toml"):
            path = ancestor / ".cargo" / name
            if path.is_file():
                result[f"{label}/.cargo/{name}"] = file_digest(path)
    for name in ("config", "config.toml"):
        path = cargo_home / name
        if path.is_file():
            result[f"cargo-home/{name}"] = file_digest(path)
    return result


def configuration(root: Path, cargo_config_dir: Path | None = None) -> dict[str, object]:
    files = {name: file_digest(root / name) for name in CONFIG_FILES}
    for directory in (root, root / "apps", root / "apps/maple-agent"):
        for name in ("rust-toolchain", "rust-toolchain.toml"):
            path = directory / name
            if path.is_file():
                files[str(path.relative_to(root))] = file_digest(path)
    cargo_home = Path(os.environ["CARGO_HOME"]).expanduser() if os.environ.get("CARGO_HOME") else Path.home() / ".cargo"
    configs = {"workspace": cargo_configuration(root / "apps/maple-agent", root, cargo_home)}
    if cargo_config_dir is not None:
        # The provider suite executes Cargo from its copied upstream source,
        # with a distinct hierarchy. Fingerprint after preparation so any
        # upstream .cargo configuration is present before target-cache restore.
        configs["suite"] = cargo_configuration(cargo_config_dir, root, cargo_home)
    return {"files": files, "manifests": manifest_settings(root), "cargo_configs": configs}


def build_identity(profile: str, root: Path = ROOT, cargo_config_dir: Path | None = None) -> dict[str, object]:
    system = platform.system()
    rustc = executable(os.environ.get("RUSTC", os.environ.get("CARGO_BUILD_RUSTC", "rustc")))
    cargo = executable("cargo")
    if system != "Windows" and (not rustc.startswith("/nix/store/") or not cargo.startswith("/nix/store/")):
        raise ValueError("fingerprint Agent targets inside the component's pinned Nix shell")
    environment = dict(os.environ)
    if system == "Windows":
        canonical = {key.upper(): key for key in ENV_KEYS}
        environment = {canonical.get(key.upper(), key.upper()): value for key, value in environment.items()}
    if profile == "release" and environment.get("MAPLE_RELEASE_PROFILE"):
        variant = environment["MAPLE_RELEASE_PROFILE"]
        if variant not in ("dev", "prod"):
            raise ValueError("unsupported embedded Agent release profile")
        # build-release.sh applies these values after cache restore. Record the
        # effective build settings here as well as hashing that script below.
        environment["VITE_OPEN_SECRET_PCR_ENVIRONMENT"] = "development" if variant == "dev" else "production"
        if system == "Darwin":
            environment["MACOSX_DEPLOYMENT_TARGET"] = "15.0"
    selected_environment = {
        key: value for key, value in environment.items()
        if key in ENV_KEYS or key.startswith(ENV_PREFIXES)
    }
    return {
        "schema": SCHEMA, "profile": profile, "features": PROFILES[profile],
        "platform": system, "architecture": platform.machine(),
        "runner_os": environment.get("RUNNER_OS"), "runner_arch": environment.get("RUNNER_ARCH"),
        "rustc": {"path": rustc, "version": command([rustc, "-vV"])},
        "cargo": {"path": cargo, "version": command([cargo, "-V"])},
        # Cargo resolves external subcommands separately from rustc. Record the
        # compiler-driving Clippy actually used by the Linux CI feature matrix.
        "clippy": command([cargo, "clippy", "--version"]) if system == "Linux" and profile == "ci" else None,
        "native": native_identity(system, environment),
        "native_build_tools": native_build_tools(environment),
        "environment": selected_environment, "configuration": configuration(root, cargo_config_dir),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=PROFILES, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cargo-config-dir", type=Path, help="also fingerprint Cargo configuration at this invocation directory")
    arguments = parser.parse_args()
    identity = build_identity(arguments.profile, cargo_config_dir=arguments.cargo_config_dir)
    compatibility = digest(identity)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps({"compatibility": compatibility, **identity}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if os.environ.get("GITHUB_OUTPUT"):
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write(f"compatibility={compatibility}\n")
    print(f"Agent {arguments.profile} cache compatibility: {compatibility}")
    print(identity["rustc"]["version"])
    print(f"Native environment: {identity['platform']}/{identity['architecture']}; details: {arguments.output}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Agent cache fingerprint failed: {error}", file=sys.stderr)
        sys.exit(1)
