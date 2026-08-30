#!/usr/bin/env bash
# Scenario 10 — leaderElection.enabled=false behaves like before (plus Recreate);
# the default path creates a Lease.
export SCEN=10-optout; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b --set leaderElection.enabled=false; apply_cr
k -n "$NS_OP" get lease "${RELEASE}-leader" >/dev/null 2>&1 && fail "lease must not exist when opted out"
k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.spec.template.spec.containers[0].env[*].name}' | grep -q LEADER_ELECTION && fail "LEADER_ELECTION env rendered when opted out"
[ "$(strategy_type)" = RollingUpdate ] || fail "strategy $(strategy_type)"
helm template x "$CHART_OLD" | python3 -c 'import sys,yaml; docs=[d for d in yaml.safe_load_all(sys.stdin) if d and d.get("kind")=="ClusterRole"]; print(sorted(str(r) for r in docs[0]["rules"]))' > "$EVID/$SCEN/rules-old.txt"
helm template x "$CHART_FIX" --set leaderElection.enabled=false | python3 -c 'import sys,yaml; docs=[d for d in yaml.safe_load_all(sys.stdin) if d and d.get("kind")=="ClusterRole"]; print(sorted(str(r) for r in docs[0]["rules"]))' > "$EVID/$SCEN/rules-new.txt"
diff "$EVID/$SCEN/rules-old.txt" "$EVID/$SCEN/rules-new.txt" >/dev/null || fail "ClusterRole rules differ from 0.2.22 when opted out"
pod=$(op_pod_names | head -1)
for _ in $(seq 1 20); do op_logs "$pod" | grep -q 'Leader election disabled' && break; sleep 1; done
op_logs "$pod" | grep -q 'Leader election disabled' || fail "missing 'Leader election disabled' log"
wait_for 120 img_is "$(img_new)" >/dev/null || fail "precondition"
[ "$(readyz_of "$pod")" = leader ] || fail "readyz opted out: $(readyz_of "$pod")"
operator_install "$CHART_FIX" 0.2.23-a --set leaderElection.enabled=false
r=$(wait_for 30 img_is "$(img_old)") || fail "opt-out upgrade left a stale CronJob"
evidence "opt-out: no lease, RBAC unchanged, upgrade ok ($r)"
pass "opt-out compatibility"
operator_install "$CHART_FIX" 0.2.23-b
wait_for 20 has_holder >/dev/null || fail "default install did not create a lease"
evidence "default install: lease $(lease_state)"
pass "default install creates the lease"
