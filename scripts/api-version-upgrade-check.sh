#!/usr/bin/env bash
# Pre-release check for the CRD API versions and the Helm CRD upgrade path
# (docs/api-stability.md, README "CRD upgrades"). Runs on a throwaway kind
# cluster; no Strimzi or Kafka needed — only the CRDs and the chart mechanics
# are exercised.
#
#   scripts/api-version-upgrade-check.sh [previous-chart-git-ref]   (default v0.2.25)
#
# Proves, against the chart in the working tree:
#   1. the previous release's chart installs only v1alpha1 (static crds/),
#   2. a plain `helm upgrade` is refused with the ownership error the README
#      documents, and `helm upgrade --take-ownership` succeeds,
#   3. the CRDs then serve v1 (storage) and v1alpha1 (deprecated, warning printed),
#      keep the helm.sh/resource-policy annotation, and a pre-existing v1alpha1
#      object is served as v1,
#   4. `helm uninstall` keeps the CRDs and resources; a fresh install and
#      `crds.install=false` both work.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PREV_REF="${1:-v0.2.25}"
CLUSTER="sbo-apicheck"
NS="kafka"
WORK="$(mktemp -d)"
FAILED=0
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAILED=1; }
cleanup() { kind delete cluster --name "$CLUSTER" >/dev/null 2>&1 || true; rm -rf "$WORK"; }
trap cleanup EXIT

for tool in kind kubectl helm git; do command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 2; }; done
PREV_TAG="${PREV_REF#v}"
HELM_OPTS=(--set leaderElection.enabled=false)

kind create cluster --name "$CLUSTER" --wait 120s >/dev/null 2>&1 || { echo "error: kind cluster" >&2; exit 2; }
kubectl create namespace "$NS" >/dev/null 2>&1

# 1. previous release's chart
mkdir -p "$WORK/prev"
git archive "$PREV_REF" deploy/helm/strimzi-backup-operator | tar -x -C "$WORK/prev"
helm install sbo "$WORK/prev/deploy/helm/strimzi-backup-operator" -n "$NS" --set image.tag="$PREV_TAG" "${HELM_OPTS[@]}" >/dev/null 2>&1 \
  && pass "helm install $PREV_REF" || fail "helm install $PREV_REF"
[ "$(kubectl get crd kafkabackups.kafkabackup.com -o jsonpath='{.spec.versions[*].name}')" = "v1alpha1" ] \
  && pass "$PREV_REF serves only v1alpha1" || fail "$PREV_REF versions"
sed 's#^apiVersion: kafkabackup.com/v1$#apiVersion: kafkabackup.com/v1alpha1#' manifests/e2e/kafkabackup-incr.yaml > "$WORK/kb-alpha.yaml"
kubectl apply -n "$NS" -f "$WORK/kb-alpha.yaml" >/dev/null 2>&1 && pass "v1alpha1 object created on $PREV_REF" || fail "v1alpha1 apply"
NAME="$(kubectl get kafkabackups -n "$NS" -o jsonpath='{.items[0].metadata.name}')"

# 2. upgrade path
if helm upgrade sbo deploy/helm/strimzi-backup-operator -n "$NS" --set image.tag="$PREV_TAG" "${HELM_OPTS[@]}" > "$WORK/plain.log" 2>&1; then
  fail "plain helm upgrade unexpectedly succeeded (CRD ownership check missing?)"
else
  grep -q "invalid ownership metadata" "$WORK/plain.log" && pass "plain helm upgrade refuses with the documented ownership error" || { fail "plain upgrade failed for another reason"; tail -3 "$WORK/plain.log"; }
fi
helm upgrade sbo deploy/helm/strimzi-backup-operator -n "$NS" --set image.tag="$PREV_TAG" "${HELM_OPTS[@]}" --take-ownership > "$WORK/upgrade.log" 2>&1 \
  && pass "helm upgrade --take-ownership" || { fail "helm upgrade --take-ownership"; tail -3 "$WORK/upgrade.log"; }

# 3. versions after upgrade
STORAGE="$(kubectl get crd kafkabackups.kafkabackup.com -o jsonpath='{range .spec.versions[*]}{.name}={.storage} {end}')"
[[ "$STORAGE" == *"v1=true"* && "$STORAGE" == *"v1alpha1=false"* ]] && pass "v1 is storage, v1alpha1 served ($STORAGE)" || fail "storage flags: $STORAGE"
kubectl get crd kafkabackups.kafkabackup.com -o jsonpath='{.metadata.annotations.helm\.sh/resource-policy}' | grep -q keep \
  && pass "helm.sh/resource-policy: keep" || fail "keep annotation"
AV="$(kubectl get kafkabackups.v1.kafkabackup.com -n "$NS" "$NAME" -o jsonpath='{.apiVersion}' 2>/dev/null)"
[ "$AV" = "kafkabackup.com/v1" ] && pass "pre-existing v1alpha1 object is served as v1" || fail "served as $AV"
kubectl get kafkabackups.v1alpha1.kafkabackup.com -n "$NS" 2>&1 >/dev/null | grep -qi "deprecated" \
  && pass "v1alpha1 requests print the deprecation warning" || fail "no deprecation warning"
kubectl apply -n "$NS" -f manifests/e2e/kafkabackup-incr.yaml >/dev/null 2>&1 && pass "same object re-applied as v1" || fail "v1 apply"

# 4. uninstall keeps, fresh install, crds.install=false
helm uninstall sbo -n "$NS" >/dev/null 2>&1
kubectl get crd kafkabackups.kafkabackup.com >/dev/null 2>&1 && pass "CRDs survive helm uninstall" || fail "CRDs deleted on uninstall"
kubectl get kafkabackups.v1.kafkabackup.com -n "$NS" "$NAME" >/dev/null 2>&1 && pass "resources survive helm uninstall" || fail "resources deleted"
kubectl delete crd kafkabackups.kafkabackup.com kafkarestores.kafkabackup.com >/dev/null 2>&1
helm install sbo deploy/helm/strimzi-backup-operator -n "$NS" --set image.tag="$PREV_TAG" "${HELM_OPTS[@]}" >/dev/null 2>&1 \
  && pass "fresh install" || fail "fresh install"
[ "$(kubectl get crd kafkabackups.kafkabackup.com -o jsonpath='{.spec.versions[*].name}')" = "v1 v1alpha1" ] \
  && pass "fresh install serves v1 v1alpha1" || fail "fresh install versions"
helm install sbo2 deploy/helm/strimzi-backup-operator -n "$NS" --set crds.install=false --set fullnameOverride=sbo2 --set image.tag="$PREV_TAG" "${HELM_OPTS[@]}" >/dev/null 2>&1 \
  && pass "crds.install=false installs alongside" || fail "crds.install=false"

[ "$FAILED" -eq 0 ] && echo "API version upgrade check: OK" || { echo "API version upgrade check: FAILED"; exit 1; }
