#!/usr/bin/env bash
# Move the operator's default kafka-backup engine image to a new release.
#
#   scripts/bump-engine.sh v0.19.2
#
# Rewrites the compiled-in constant and the README "Current release" line,
# and inserts a CHANGELOG stub under the version currently in Cargo.toml.
# Prints the remaining manual steps (docs repo). Run
# scripts/check-engine-image.sh afterwards; the release gate runs it too.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

die() {
  echo "error: $*" >&2
  exit 1
}

NEW_TAG="${1:-}"
[[ "$NEW_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: $0 vX.Y.Z"
NEW_IMAGE="osodevops/kafka-backup:${NEW_TAG}"
OLD_IMAGE="$(scripts/default-engine-image.sh)"
[[ "$OLD_IMAGE" != "$NEW_IMAGE" ]] || die "default is already ${NEW_IMAGE}"

VERSION="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"

sed_i() {
  if sed --version >/dev/null 2>&1; then sed -i "$@"; else sed -i '' "$@"; fi
}

sed_i "s|^pub const DEFAULT_BACKUP_IMAGE: &str = \"${OLD_IMAGE}\";|pub const DEFAULT_BACKUP_IMAGE: \&str = \"${NEW_IMAGE}\";|" src/engine.rs
sed_i "s|^\(\*\*Current release: [0-9.]*\*\* — default job image \`\)${OLD_IMAGE}\(\`\.\)$|\1${NEW_IMAGE}\2|" README.md

grep -q "\"${NEW_IMAGE}\"" src/engine.rs || die "failed to update src/engine.rs"
grep -q "default job image \`${NEW_IMAGE}\`" README.md || die "failed to update README.md"

# CHANGELOG stub under the current version's section (create the section if
# this is the first entry for it).
python3 - "$VERSION" "$NEW_IMAGE" "$OLD_IMAGE" <<'PY'
import re, sys, datetime
version, new_image, old_image = sys.argv[1:4]
p = "CHANGELOG.md"
s = open(p).read()
stub = (
    f"- Update the default job image to `{new_image}` (was `{old_image}`).\n"
    f"  TODO: say what changed in the engine and whether existing archives are\n"
    f"  affected. Pin `spec.image` to keep an older image.\n"
)
heading = re.search(rf"^## {re.escape(version)}(?: - \d{{4}}-\d{{2}}-\d{{2}})?\n", s, re.M)
if heading is None:
    today = datetime.date.today().isoformat()
    block = f"## {version} - {today}\n\n### Changed\n\n{stub}\n"
    s = re.sub(r"^(# Changelog\n\n.*?\n\n)", lambda m: m.group(1) + block, s, count=1, flags=re.S)
else:
    end = heading.end()
    section_end = re.search(r"^## ", s[end:], re.M)
    section = s[end:end + section_end.start()] if section_end else s[end:]
    changed = re.search(r"^### Changed\n\n", section, re.M)
    if changed:
        insert_at = end + changed.end()
        s = s[:insert_at] + stub + s[insert_at:]
    else:
        s = s[:end] + "\n### Changed\n\n" + stub + s[end:]
open(p, "w").write(s)
PY

scripts/check-engine-image.sh

cat <<MSG

Bumped default engine image: ${OLD_IMAGE} -> ${NEW_IMAGE}

Next:
  1. Replace the TODO in CHANGELOG.md with what changed in kafka-backup ${NEW_TAG}.
  2. Add a row for operator ${VERSION} to the README "Compatibility" table.
  3. After release: sweep the docs repo (kafka-backup-docs):
       docs/docs/strimzi-operator/index.md   (current release / default job image)
       docs/docs/strimzi-operator/metrics.md
       docs/docs/intro.md                    (What's New)
MSG
