#!/usr/bin/env python3
"""Read only public client settings; never source a dotenv file or read credentials."""

import argparse
import os
from pathlib import Path
import tempfile
from urllib.parse import urlsplit
from uuid import UUID


PUBLIC_KEYS = {
    "VITE_OPEN_SECRET_API_URL": "MAPLE_API_URL",
    "VITE_MAPLE_BILLING_API_URL": "MAPLE_BILLING_URL",
    "VITE_OS_FLAGS_BASE_URL": "MAPLE_FLAGS_URL",
}


def loopback_origin(value: str) -> str:
    if any(char.isspace() or ord(char) < 32 for char in value):
        raise ValueError("Local service URLs must be explicit HTTP loopback origins.")
    try:
        url = urlsplit(value)
        valid = (
            url.scheme == "http"
            and url.hostname in {"127.0.0.1", "localhost", "::1"}
            and url.port is not None
            and 0 < url.port <= 65535
            and url.username is None
            and url.password is None
            and url.path in {"", "/"}
            and not url.query
            and not url.fragment
        )
    except ValueError:
        valid = False
    if not valid:
        raise ValueError("Local service URLs must be explicit HTTP loopback origins.")
    return value.rstrip("/")


def read_public_values(path: Path) -> dict[str, str]:
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        key = key.strip()
        if not separator or key not in PUBLIC_KEYS:
            continue
        if key in values:
            raise ValueError("The input contains duplicate public endpoint settings.")
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        values[key] = loopback_origin(value)
    if values.keys() != PUBLIC_KEYS.keys():
        raise ValueError("The input must define API, billing and flags public endpoints.")
    return {PUBLIC_KEYS[key]: value for key, value in values.items()}


def configure(env_file: Path, client_id: str, auth_url: str, output: Path) -> None:
    values = read_public_values(env_file)
    values["MAPLE_AUTH_URL"] = loopback_origin(auth_url)
    try:
        values["MAPLE_CLIENT_ID"] = str(UUID(client_id))
    except ValueError:
        raise ValueError("The public client ID must be a UUID.") from None
    values["MAPLE_PROFILE"] = "local"
    lines = ["// Generated public local settings. No account or provider credentials."]
    # xcconfig treats // as a comment; the empty expansion preserves URL slashes.
    lines.extend(f"{key} = {value.replace('://', ':/$()/')}" for key, value in sorted(values.items()))
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.is_symlink():
        raise ValueError("The output must not be a symbolic link.")
    descriptor, temporary = tempfile.mkstemp(prefix=".local-config-", dir=output.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write("\n".join(lines) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--env-file", type=Path, required=True)
    parser.add_argument("--client-id", required=True, help="Public application client UUID, never an account credential")
    parser.add_argument("--auth-url", required=True, help="Explicit loopback web authentication origin")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent.parent / "Config/Local.generated.xcconfig")
    arguments = parser.parse_args()
    try:
        configure(arguments.env_file, arguments.client_id, arguments.auth_url, arguments.output)
    except (OSError, UnicodeError, ValueError) as error:
        # Do not echo source lines, environment values, or paths from untrusted input.
        message = str(error) if isinstance(error, ValueError) else "Could not read or write local configuration."
        parser.exit(1, message + "\n")
    print("Local public configuration written. Account credentials are not included.")


if __name__ == "__main__":
    main()
