#!/usr/bin/env bash
# Scenario 6 — renewals fail (leases RBAC removed): leader exits within
# renewDeadline, restarted pod is NotReady until RBAC is restored.
export SCEN=06-renew-failure; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b; apply_cr
wait_for 30 has_holder >/dev/null || fail "no leader"
pod=$(lease_holder)
k get clusterrole "$RELEASE" -o json > "$EVID/$SCEN/clusterrole.json"
IDX=$(python3 -c 'import json,sys; rs=json.load(open(sys.argv[1]))["rules"]; print([i for i,r in enumerate(rs) if "leases" in r.get("resources",[])][0])' "$EVID/$SCEN/clusterrole.json")
k patch clusterrole "$RELEASE" --type=json -p="[{\"op\":\"remove\",\"path\":\"/rules/$IDX\"}]" >/dev/null
t_break=$(date +%s); log "leases RBAC removed"
r=$(wait_for 30 restarts_ge "$pod" 1) || fail "leader did not exit/restart after losing renew permission"
took=$(( $(date +%s) - t_break )); [ "$took" -le 16 ] || fail "exit took ${took}s (> renewDeadline 10s + retry + kubelet)"
exit_code=$(pod_times | grep "^$pod" | grep -o 'lastExit=[0-9-]*' | cut -d= -f2); [ "$exit_code" = 1 ] || fail "exit code $exit_code, expected 1"
k -n "$NS_OP" logs "$pod" --previous 2>/dev/null > "$EVID/$SCEN/leader-previous.log" || true
grep -q "Leader election failed" "$EVID/$SCEN/leader-previous.log" || fail "previous log lacks the leader-election failure line"
grep -q "renew deadline" "$EVID/$SCEN/leader-previous.log" || fail "previous log lacks the renew deadline reason"
wait_for 15 readyz_unavailable "$pod" >/dev/null || fail "restarted pod should be NotReady: $(readyz_of "$pod")"
sleep 3
op_logs "$pod" | grep -q "Reconciling KafkaBackup" && fail "restarted pod reconciled without holding the lease"
k patch clusterrole "$RELEASE" --type=json -p='[{"op":"add","path":"/rules/-","value":{"apiGroups":["coordination.k8s.io"],"resources":["leases"],"verbs":["get","list","watch","create","update","patch","delete"]}}]' >/dev/null; t_fix=$(date +%s); log "leases RBAC restored"
r=$(wait_for 15 holder_is "$pod") || fail "did not re-acquire after RBAC restore"
wait_for 10 readyz_is "$pod" leader >/dev/null || fail "readyz after recovery: $(readyz_of "$pod")"
c0=$(reconcile_count "$pod"); touch_cr; wait_for 15 reconciles_gt "$pod" "$c0" >/dev/null || fail "no reconcile after recovery"
evidence "renew failure: exit after ${took}s (code $exit_code), recovered $r after RBAC restore"
pass "renew failure -> exit -> recover"
