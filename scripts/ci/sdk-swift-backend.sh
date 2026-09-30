#!/usr/bin/env bash
# Fresh hosted CI runner only. Local developer environments retain their own
# service/database lifecycle; use sdk-swift.sh integration against that endpoint.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo "This fixture bootstrap is for disposable CI runners only." >&2; exit 2; }
: "${RUNNER_TEMP:?A runner-owned temporary directory is required}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
state="$(mktemp -d "$RUNNER_TEMP/maple-swift-integration.XXXXXX")"
backend_pid=""
database_started=0
cleanup() {
  if [[ -n "$backend_pid" ]]; then
    kill "$backend_pid" 2>/dev/null || true
    wait "$backend_pid" 2>/dev/null || true
  fi
  if [[ "$database_started" == 1 ]]; then pg_ctl -D "$state/postgres" stop -m fast -w >/dev/null; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Values below are public, synthetic fixtures, as in sdk-integration.yml.
export APP_MODE=local
export DATABASE_URL=postgres://opensecret_user:password@127.0.0.1:35432/opensecret
export OPENSECRET_DEV_DATABASE_URL="$DATABASE_URL"
export OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0
export ENCLAVE_SECRET_MOCK=1111111111111111111111111111111111111111111111111111111111111111
export JWT_SECRET=2222222222222222222222222222222222222222222222222222222222222222
export OPENAI_API_BASE=http://127.0.0.1:9
export TINFOIL_API_KEY=sdk-integration-placeholder
export OPENSECRET_BIND_ADDR=127.0.0.1:35433
export RUST_LOG=opensecret=warn
export MAPLE_SWIFT_INTEGRATION_URL=http://127.0.0.1:35433
export MAPLE_SWIFT_TEST_EMAIL=sdk-swift-ci-user@local.test
export MAPLE_SWIFT_TEST_PASSWORD=Maple-sdk-ci-test-1234
export MAPLE_SWIFT_TEST_CLIENT_ID=ba5a14b5-d915-47b1-b7b1-afda52bc5fc6
export MAPLE_SWIFT_TEST_INVITE_CODE=11111111-1111-4111-8111-111111111111

initdb -D "$state/postgres" --auth=trust --username=opensecret_user > "$state/initdb.log"
pg_ctl -D "$state/postgres" -l "$state/postgres.log" \
  -o "-h 127.0.0.1 -p 35432 -k $state" start -w >/dev/null
database_started=1
createdb -h 127.0.0.1 -p 35432 -U opensecret_user opensecret
(
  cd "$repo_root/services/opensecret"
  diesel migration run
  cargo build --locked
)
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f "$repo_root/sdk/test/integration/bootstrap.sql"
"$repo_root/services/opensecret/target/debug/opensecret" > "$state/backend.log" 2>&1 &
backend_pid=$!
ready=0
for _ in $(seq 1 60); do
  if curl --fail --silent --show-error --max-time 2 "$MAPLE_SWIFT_INTEGRATION_URL/health-check" >/dev/null 2>&1; then
    ready=1; break
  fi
  kill -0 "$backend_pid" 2>/dev/null || break
  sleep 2
done
[[ "$ready" == 1 ]] || { echo "Disposable OpenSecret failed to become ready; inspect the runner-local fixture logs." >&2; exit 1; }
# No provider-spending path, hosted trust opt-in, or secret-bearing log upload.
env -u RUN_LIVE_AI -u MAPLE_RUN_LIVE_ATTESTATION -u MAPLE_SWIFT_DEV_INTEGRATION \
  bash "$repo_root/scripts/ci/sdk-swift.sh" integration
