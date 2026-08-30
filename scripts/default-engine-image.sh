#!/usr/bin/env bash
# Print the kafka-backup engine image compiled into this operator
# (`DEFAULT_BACKUP_IMAGE` in src/engine.rs). Single source of truth for the
# release gate, CI and scripts/bump-engine.sh.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
awk -F '"' '/^pub const DEFAULT_BACKUP_IMAGE: &str = / { print $2; exit }' "$ROOT_DIR/src/engine.rs"
