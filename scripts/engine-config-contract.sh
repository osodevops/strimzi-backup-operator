#!/usr/bin/env bash
# Run the operator's generated backup/restore configs through a kafka-backup
# engine image and fail if the engine reports a config key it does not know.
#
#   scripts/engine-config-contract.sh <image> <fixtures-dir>
#
# The fixtures are written by the adapter unit tests when
# WRITE_CONFIG_FIXTURES=<dir> is set (see src/adapters/*_config.rs). The
# engine parses the config and warns about unknown keys *before* it connects
# to Kafka or storage, so the (expected) connection failure exit code is
# ignored and only the warning matters. Once kafka-backup ships
# `config check` (osodevops/kafka-backup#171) this script should use it.
set -euo pipefail

IMAGE="${1:-}"
FIXTURES="${2:-}"
[[ -n "$IMAGE" && -d "$FIXTURES" ]] || {
  echo "usage: $0 <image> <fixtures-dir>" >&2
  exit 2
}

FIXTURES="$(cd "$FIXTURES" && pwd)"
status=0
for fixture in "$FIXTURES"/*.yaml; do
  name="$(basename "$fixture" .yaml)"
  mode="${name%%-*}" # backup-full.yaml -> backup, restore-full.yaml -> restore
  log="$(mktemp)"
  echo "== ${IMAGE} ${mode} --config ${name}.yaml"
  docker run --rm -v "${FIXTURES}:/fixtures:ro" "$IMAGE" \
    "$mode" --config "/fixtures/${name}.yaml" >"$log" 2>&1 || true
  if grep -q 'Ignoring unknown config key' "$log"; then
    echo "::error title=Engine does not understand the operator's config::${IMAGE} rejected keys in ${name}.yaml"
    grep 'Ignoring unknown config key' "$log" >&2
    status=1
  elif ! grep -q -E 'Ignoring unknown config key|bootstrap|connect|storage|manifest|backup_id|Failed|error' "$log"; then
    # The engine must at least have got past parsing; an empty log means the
    # image did not run the CLI at all.
    echo "::error::${IMAGE} produced no recognisable output for ${name}.yaml" >&2
    sed -n '1,40p' "$log" >&2
    status=1
  else
    echo "   ok — parsed without unknown-key warnings"
  fi
  rm -f "$log"
done
exit $status
