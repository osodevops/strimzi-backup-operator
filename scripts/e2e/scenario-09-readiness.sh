#!/usr/bin/env bash
# Scenario 9 — `helm upgrade --wait` completes for every supported topology.
export SCEN=09-readiness; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
timed() { local t0=$(date +%s); "$@"; echo $(( $(date +%s) - t0 )); }
check_progress() { [ "$(k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.status.conditions[?(@.type=="Progressing")].reason}')" = NewReplicaSetAvailable ] || fail "$1: Progressing reason"; }
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-a; apply_cr
t=$(timed operator_install "$CHART_FIX" 0.2.23-b); check_progress default; log "default (maxSurge 0) upgrade --wait: ${t}s"
operator_install "$CHART_FIX" 0.2.23-a --set replicaCount=2
t=$(timed operator_install "$CHART_FIX" 0.2.23-b --set replicaCount=2); check_progress rolling-2; log "default, replicas=2 upgrade --wait: ${t}s"
operator_install "$CHART_FIX" 0.2.23-a --set replicaCount=2 --set updateStrategy.rollingUpdate.maxSurge=1 --set updateStrategy.rollingUpdate.maxUnavailable=0
t=$(timed operator_install "$CHART_FIX" 0.2.23-b --set replicaCount=2 --set updateStrategy.rollingUpdate.maxSurge=1 --set updateStrategy.rollingUpdate.maxUnavailable=0); check_progress surge-2; log "maxSurge 1 / maxUnavailable 0, replicas=2 upgrade --wait: ${t}s"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-a --set updateStrategy.type=Recreate
t=$(timed operator_install "$CHART_FIX" 0.2.23-b --set updateStrategy.type=Recreate); check_progress recreate; log "Recreate (fresh install) upgrade --wait: ${t}s"
[ "$(k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.spec.template.spec.containers[0].readinessProbe.httpGet.path}')" = /readyz ] || fail "readiness probe path"
for p in $(op_pod_names); do case "$(readyz_of "$p")" in leader|standby) ;; *) fail "$p readyz: $(readyz_of "$p")";; esac; done
evidence "all --wait upgrades completed"
pass "readiness / helm --wait"
