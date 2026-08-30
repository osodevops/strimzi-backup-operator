#!/usr/bin/env bash
# Shared helpers for the minikube e2e scenarios (issue #62).
# Every kubectl/helm call is pinned to the dedicated profile: never run unpinned.
set -euo pipefail

PROFILE="${PROFILE:-sbo-e2e}"
NS_OP="${NS_OP:-sbo}"
NS_KAFKA="${NS_KAFKA:-kafka}"
RELEASE="${RELEASE:-strimzi-backup-operator}"
IMAGE_REPO="${IMAGE_REPO:-sbo-e2e/operator}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
E2E_DIR="${E2E_DIR:-$ROOT/.e2e}"
# Engine images the scenarios drive (issue #67). The "-b" operator builds carry
# ENGINE_NEW — this checkout's compiled-in default — and the "-a" builds
# ENGINE_OLD, so an operator upgrade is observable on the CronJob image
# (issue #62). Override both to exercise another pair.
ENGINE_NEW="${ENGINE_NEW:-$("$ROOT/scripts/default-engine-image.sh")}"
ENGINE_OLD="${ENGINE_OLD:-osodevops/kafka-backup:v0.19.0}"
EVID="${EVID:-$E2E_DIR/evidence}"
SCEN="${SCEN:-misc}"
CHART_FIX="$ROOT/deploy/helm/strimzi-backup-operator"
CHART_OLD="$E2E_DIR/src-v0.2.22/deploy/helm/strimzi-backup-operator"
mkdir -p "$EVID/$SCEN"

k() { kubectl --context "$PROFILE" "$@"; }
h() { helm --kube-context "$PROFILE" "$@"; }
# Background watchers must not outlive the script (nor keep a parent pipeline alive).
stop_watchers() {
  pkill -f "get lease ${RELEASE}-leader -w" 2>/dev/null || true
  pkill -f "get pods -l app.kubernetes.io/name=strimzi-backup-operator -w" 2>/dev/null || true
  pkill -f "scripts/e2e/watch.py" 2>/dev/null || true
}
trap 'stop_watchers' EXIT
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" | tee -a "$EVID/$SCEN/log.md" >&2; }
fail() { log "FAIL: $*"; exit 1; }
pass() { log "PASS: $*"; }

now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }
to_epoch() { python3 -c 'import sys,datetime; s=sys.argv[1]; s=s.replace("Z","+00:00"); print(datetime.datetime.fromisoformat(s).timestamp())' "$1"; }

cronjob_image() { k -n "$NS_KAFKA" get cronjob incr-scheduled -o jsonpath='{.spec.jobTemplate.spec.template.spec.containers[0].image}'; }
cronjob_apply_time() { k -n "$NS_KAFKA" get cronjob incr-scheduled -o json --show-managed-fields | python3 "$ROOT/scripts/e2e/jsonq.py" apply-time; }
cronjob_managers() { k -n "$NS_KAFKA" get cronjob incr-scheduled -o json --show-managed-fields | python3 "$ROOT/scripts/e2e/jsonq.py" managers; }
op_pods() { k -n "$NS_OP" get pods -l app.kubernetes.io/name=strimzi-backup-operator -o json; }
op_pod_names() { op_pods | python3 "$ROOT/scripts/e2e/jsonq.py" pod-names; }
pod_times() { op_pods | python3 "$ROOT/scripts/e2e/jsonq.py" pod-times; }
lease_state() { k -n "$NS_OP" get lease "${RELEASE}-leader" -o jsonpath='{.spec.holderIdentity}{"\t"}{.spec.leaseTransitions}{"\t"}{.spec.acquireTime}{"\t"}{.spec.renewTime}{"\t"}{.metadata.resourceVersion}{"\n"}' 2>/dev/null || echo "<no lease>"; }
lease_holder() { k -n "$NS_OP" get lease "${RELEASE}-leader" -o jsonpath='{.spec.holderIdentity}' 2>/dev/null || true; }
lease_transitions() { k -n "$NS_OP" get lease "${RELEASE}-leader" -o jsonpath='{.spec.leaseTransitions}' 2>/dev/null || echo 0; }
metrics_of() { k get --raw "/api/v1/namespaces/$NS_OP/pods/$1:9090/proxy/metrics"; }
readyz_of() { k get --raw "/api/v1/namespaces/$NS_OP/pods/$1:9090/proxy/readyz" 2>&1 || true; }
leader_pods() { for p in $(op_pod_names); do metrics_of "$p" 2>/dev/null | grep -Eq 'strimzi_backup_operator_leader\{[^}]*\} 1$' && echo "$p"; done; true; }
reconcile_count() { metrics_of "$1" 2>/dev/null | grep -E '^strimzi_backup_operator_reconciliations_total\{controller="backup"' | awk '{s+=$2} END {print s+0}'; }
touch_cr() { k -n "$NS_KAFKA" annotate kafkabackup incr e2e/touch="$(date +%s%N)" --overwrite >/dev/null; }
strategy_type() { k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.spec.strategy.type}'; }
op_logs() { k -n "$NS_OP" logs "$1" --timestamps 2>/dev/null || true; }
save_logs() { for p in $(op_pod_names); do op_logs "$p" > "$EVID/$SCEN/$1-$p.log" 2>/dev/null || true; done; }

# wait_for <budget-seconds> <command...>  — prints "ok after Ns"
wait_for() { local budget=$1; shift; local t0=$(date +%s); until "$@" >/dev/null 2>&1; do if (( $(date +%s) - t0 > budget )); then return 1; fi; sleep 0.5; done; echo "ok after $(( $(date +%s) - t0 ))s"; }
img_is() { [ "$(cronjob_image)" = "$1" ]; }
img_old() { echo "$ENGINE_OLD"; }
img_new() { echo "$ENGINE_NEW"; }
# Engine image the CronJob must carry after installing operator build tag $1 (…-a / …-b).
img_for_tag() { case "$1" in *-a) img_old;; *-b) img_new;; *) fail "unknown build tag $1";; esac; }
patch_cronjob_image() { k -n "$NS_KAFKA" patch cronjob incr-scheduled --type=json -p="[{\"op\":\"replace\",\"path\":\"/spec/jobTemplate/spec/template/spec/containers/0/image\",\"value\":\"$1\"}]" >/dev/null; }
# "<status>/<reason>" of condition $2 on KafkaBackup $1.
cr_condition() { k -n "$NS_KAFKA" get kafkabackup "$1" -o jsonpath="{.status.conditions[?(@.type==\"$2\")].status}/{.status.conditions[?(@.type==\"$2\")].reason}"; }
# Re-evaluated predicates for wait_for (a "$(...)" in the wait_for argument list expands only once).
leader_count_is() { [ "$(leader_pods | wc -l | tr -d ' ')" = "$1" ]; }
restarts_ge() { local n; n=$(pod_times | grep "^$1" | grep -o 'restarts=[0-9]*' | cut -d= -f2); [ -n "$n" ] && [ "$n" -ge "$2" ]; }
ready_replicas_is() { [ "$(k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.status.readyReplicas}')" = "$1" ]; }
reconciles_gt() { [ "$(reconcile_count "$1")" -gt "$2" ]; }
holder_is() { [ "$(lease_holder)" = "$1" ]; }
has_holder() { [ -n "$(lease_holder)" ]; }
readyz_is() { [ "$(readyz_of "$1")" = "$2" ]; }
readyz_unavailable() { readyz_of "$1" | grep -Eq "leader election pending|ServiceUnavailable|503"; }
cronjob_suspend_is() { [ "$(k -n "$NS_KAFKA" get cronjob incr-scheduled -o jsonpath='{.spec.suspend}')" = "$1" ]; }

watch_pods_bg() { k -n "$NS_OP" get pods -l app.kubernetes.io/name=strimzi-backup-operator -w --output-watch-events -o json 2>/dev/null | python3 -u "$ROOT/scripts/e2e/watch.py" pods > "$1" 2>/dev/null & echo $!; }
watch_lease_bg() { k -n "$NS_OP" get lease "${RELEASE}-leader" -w --output-watch-events -o json 2>/dev/null | python3 -u "$ROOT/scripts/e2e/watch.py" lease > "$1" 2>/dev/null & echo $!; }

evidence() { { echo; echo "## $1 ($(date -u +%FT%TZ))"; echo "cronjob image: $(cronjob_image 2>/dev/null || echo '<none>')"; echo "managedFields:"; cronjob_managers 2>/dev/null | sed 's/^/  /'; echo "pods:"; pod_times | sed 's/^/  /'; echo "lease: $(lease_state)"; } | tee -a "$EVID/$SCEN/log.md"; }

# operator_install <chart-dir> <image-tag> [extra helm args...]
operator_install() { local chart=$1 tag=$2; shift 2
  h upgrade --install "$RELEASE" "$chart" -n "$NS_OP" --create-namespace \
    --set image.repository="$IMAGE_REPO" --set image.tag="$tag" --set image.pullPolicy=IfNotPresent \
    --set logging.level="info,kafka_backup_operator=debug" "$@" --wait --timeout 3m >/dev/null
  log "installed $RELEASE chart=$(basename "$(dirname "$chart")")/$(basename "$chart") tag=$tag args=[$*] strategy=$(strategy_type)"; }
operator_uninstall() { h uninstall "$RELEASE" -n "$NS_OP" --ignore-not-found --wait >/dev/null 2>&1 || true; k -n "$NS_OP" delete lease "${RELEASE}-leader" --ignore-not-found >/dev/null 2>&1 || true; }
apply_cr() { k apply -f "$ROOT/manifests/e2e/kafkabackup-incr.yaml" >/dev/null; }
