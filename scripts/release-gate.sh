#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

die() {
  echo "::error::$*" >&2
  exit 1
}

read_cargo_version() {
  awk -F '"' '/^version = / { print $2; exit }' Cargo.toml
}

read_lock_version() {
  awk -F '"' '
    /^\[\[package\]\]$/ { in_pkg = 0 }
    /^name = "kafka-backup-operator"$/ { in_pkg = 1 }
    in_pkg && /^version = / { print $2; exit }
  ' Cargo.lock
}

read_yaml_value() {
  local key="$1"
  local file="$2"
  awk -v key="$key" '$1 == key ":" { gsub(/"/, "", $2); print $2; exit }' "$file"
}

semver_greater_than() {
  local left="$1"
  local right="$2"
  local highest
  highest="$(printf '%s\n%s\n' "$left" "$right" | sort -V | tail -n1)"
  [[ "$highest" == "$left" && "$left" != "$right" ]]
}

VERSION="$(read_cargo_version)"
EXPECTED_VERSION="${EXPECTED_VERSION:-}"
LOCK_VERSION="$(read_lock_version)"
CHART_FILE="deploy/helm/strimzi-backup-operator/Chart.yaml"
CHART_VERSION="$(read_yaml_value version "$CHART_FILE")"
APP_VERSION="$(read_yaml_value appVersion "$CHART_FILE")"
TAG="v${VERSION}"

[[ -n "$VERSION" ]] || die "Could not read Cargo.toml version"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "Cargo.toml version is not semver: $VERSION"

if [[ -n "$EXPECTED_VERSION" && "$VERSION" != "$EXPECTED_VERSION" ]]; then
  die "Cargo.toml version ($VERSION) does not match expected version ($EXPECTED_VERSION)"
fi

[[ "$LOCK_VERSION" == "$VERSION" ]] || die "Cargo.lock package version ($LOCK_VERSION) does not match Cargo.toml ($VERSION)"
[[ "$APP_VERSION" == "$VERSION" ]] || die "Helm appVersion ($APP_VERSION) does not match Cargo.toml ($VERSION)"
[[ -n "$CHART_VERSION" ]] || die "Could not read Helm chart version"

if ! semver_greater_than "$CHART_VERSION" "$APP_VERSION" && [[ "$CHART_VERSION" != "$APP_VERSION" ]]; then
  die "Helm chart version ($CHART_VERSION) must be equal to or greater than appVersion ($APP_VERSION)"
fi

VERSION_RE="${VERSION//./\\.}"
if ! grep -Eq "^## (\\[?v?)?${VERSION_RE}(\\]?([[:space:]-]|$))" CHANGELOG.md; then
  die "CHANGELOG.md is missing a release section for $VERSION"
fi

git fetch --tags --quiet origin '+refs/tags/*:refs/tags/*' || git fetch --tags --quiet || true

ALLOW_EXISTING_TAG="${ALLOW_EXISTING_TAG:-false}"
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  [[ "$ALLOW_EXISTING_TAG" == "true" ]] || die "Release tag ${TAG} already exists"
else
  LATEST_TAG="$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1 || true)"
  if [[ -n "$LATEST_TAG" ]]; then
    LATEST_VERSION="${LATEST_TAG#v}"
    semver_greater_than "$VERSION" "$LATEST_VERSION" || die "Cargo.toml version ($VERSION) must be greater than latest tag ($LATEST_TAG)"
  fi
fi

CHECK_HELM_CHART_TAG="${CHECK_HELM_CHART_TAG:-true}"
if [[ "$CHECK_HELM_CHART_TAG" == "true" ]]; then
  HELM_TAG="strimzi-backup-operator-${CHART_VERSION}"
  if git ls-remote --exit-code --tags https://github.com/osodevops/helm-charts.git "refs/tags/${HELM_TAG}" >/dev/null 2>&1; then
    die "Helm chart tag ${HELM_TAG} already exists in osodevops/helm-charts"
  fi
fi

echo "Release gate passed:"
echo "  operator version: ${VERSION}"
echo "  release tag: ${TAG}"
echo "  chart version: ${CHART_VERSION}"
echo "  chart appVersion: ${APP_VERSION}"
