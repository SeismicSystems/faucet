#!/usr/bin/env bash
set -euo pipefail

ENV_FILE="/etc/faucet/machine-funding.env"
test "$(stat -c '%a:%U' "${ENV_FILE}")" = "600:faucet-funding"

set -a
source "${ENV_FILE}"
set +a

exec /usr/local/bin/faucet-machine-funding
