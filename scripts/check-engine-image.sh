#!/usr/bin/env bash
# Verify every place that states the default engine image agrees with the
# compiled-in constant, so a bump cannot land half-done (issue #67).
#
#   - README.md line "Current release: X — default job image `…`"
#   - no other tracked source/doc/manifest file pins a different
#     osodevops/kafka-backup tag (CHANGELOG is history and is exempt)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

die() {
  echo "::error::$*" >&2
  exit 1
}

IMAGE="$(scripts/default-engine-image.sh)"
[[ -n "$IMAGE" ]] || die "could not read DEFAULT_BACKUP_IMAGE from src/engine.rs"
[[ "$IMAGE" =~ ^osodevops/kafka-backup:v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "DEFAULT_BACKUP_IMAGE is not a pinned release tag: $IMAGE"

grep -Eq "^\*\*Current release: [0-9.]+\*\* — default job image \`${IMAGE}\`\.$" README.md \
  || die "README.md 'Current release' line does not name the default job image ${IMAGE}"

# Any other pinned kafka-backup tag in docs, manifests or scripts is stale
# documentation or something that will break on the next bump. Rust sources
# are exempt (test fixtures name old releases on purpose) except the CRDs,
# whose descriptions must not embed a tag at all.
# Exempt: CHANGELOG (history), scripts/e2e (pins old releases on purpose),
# workflows (render deliberate overrides), and lines marked "# example".
stale="$(git ls-files -- '*.md' '*.yaml' '*.yml' '*.sh' '*.toml' 'src/crd/*.rs' \
  | grep -v -E '^(CHANGELOG\.md|scripts/e2e/|\.github/)' \
  | xargs grep -n -E 'osodevops/kafka-backup:v[0-9]+\.[0-9]+\.[0-9]+' \
  | grep -v -E '# example' \
  | grep -v -F "${IMAGE}" || true)"
if [[ -n "$stale" ]]; then
  echo "$stale" >&2
  die "files above pin a kafka-backup tag other than the default ${IMAGE}"
fi

if git ls-files -- 'src/crd/*.rs' 'deploy/crds/*.yaml' 'deploy/helm/*/crds/*.yaml' \
  | xargs grep -l -E 'osodevops/kafka-backup:v[0-9]' >/dev/null 2>&1; then
  die "CRD descriptions must not embed the engine tag (issue #67); point at README \"Compatibility\" instead"
fi

echo "engine image check passed: ${IMAGE}"
