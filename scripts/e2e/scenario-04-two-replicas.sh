#!/usr/bin/env bash
# Scenario 4 — two replicas: exactly one leader, both Ready, graceful failover.
export SCEN=04-two-replicas; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b --set replicaCount=2; apply_cr
wait_for 120 img_is "$(img_new)" >/dev/null || fail "precondition"
wait_for 30 ready_replicas_is 2 >/dev/null || fail "both replicas must become Ready"
leaders=$(leader_pods | wc -l | tr -d ' '); [ "$leaders" = 1 ] || fail "expected exactly 1 leader, got $leaders: $(leader_pods)"
leader=$(leader_pods); standby=$(op_pod_names | grep -v "^$leader$" | head -1)
[ "$(lease_holder)" = "$leader" ] || fail "lease holder $(lease_holder) != gauge leader $leader"
[ "$(readyz_of "$leader")" = leader ] || fail "leader readyz: $(readyz_of "$leader")"
[ "$(readyz_of "$standby")" = standby ] || fail "standby readyz: $(readyz_of "$standby")"
r1=$(lease_state | cut -f4); sleep 3; r2=$(lease_state | cut -f4); [ "$r1" != "$r2" ] || fail "renewTime not advancing"
touch_cr; sleep 4
op_logs "$standby" | grep -q "Reconciling KafkaBackup" && fail "standby reconciled"
op_logs "$leader" | grep -q "Reconciling KafkaBackup" || fail "leader did not reconcile"
evidence "steady state: leader=$leader standby=$standby"
LEASELOG=$(watch_lease_bg "$EVID/$SCEN/lease-failover.jsonl"); sleep 1
tr_before=$(lease_transitions); t0=$(date +%s%N)
k -n "$NS_OP" delete pod "$leader" --wait=false >/dev/null
# Either the standby or the Deployment's replacement pod may win the release:
# what matters is that a *different* replica holds the lease within seconds.
holder_changed() { local h; h=$(lease_holder); [ -n "$h" ] && [ "$h" != "$leader" ]; }
r=$(wait_for 20 holder_changed) || fail "no other replica acquired within 20s (holder=$(lease_holder))"
new_leader=$(lease_holder); log "lease moved $leader -> $new_leader ($( [ "$new_leader" = "$standby" ] && echo the standby || echo the replacement pod ))"
sleep 2; kill "$LEASELOG" 2>/dev/null || true
summary=$(python3 "$ROOT/scripts/e2e/analyze.py" lease "$EVID/$SCEN/lease-failover.jsonl"); log "failover lease: $summary"
handover=$(echo "$summary" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("handover_s"))')
python3 -c "import sys; sys.exit(0 if float('$handover') <= 5 else 1)" || fail "graceful failover took ${handover}s (> 5s)"
[ "$(lease_transitions)" = "$((tr_before+1))" ] || fail "leaseTransitions $tr_before -> $(lease_transitions)"
wait_for 60 leader_count_is 1 >/dev/null || fail "leader count after failover"
c0=$(reconcile_count "$new_leader"); touch_cr
wait_for 15 reconciles_gt "$new_leader" "$c0" >/dev/null || fail "new leader did not reconcile after a CR touch"
evidence "after failover: new leader $(lease_holder), handover ${handover}s ($r)"
pass "two replicas + graceful failover (${handover}s)"
