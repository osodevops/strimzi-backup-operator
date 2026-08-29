#!/usr/bin/env bash
# Scenario 3 — fixed build, surge rollout (maxSurge 1 / maxUnavailable 0) +
# leader election, one replica: the incoming pod comes up next to the leader,
# stands by (Ready) and only acquires the lease when the outgoing pod releases
# it on shutdown.
export SCEN=03-rolling-le; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
N=${N:-2}
operator_uninstall
SURGE=(--set updateStrategy.type=RollingUpdate --set updateStrategy.rollingUpdate.maxSurge=1 --set updateStrategy.rollingUpdate.maxUnavailable=0)
operator_install "$CHART_FIX" 0.2.23-a "${SURGE[@]}"; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.0 >/dev/null || fail "precondition"
wait_for 20 has_holder >/dev/null || fail "no lease holder"
evidence "start: lease $(lease_state)"
from=0.2.23-a
for i in $(seq 1 "$N"); do
  to=$([ "$from" = 0.2.23-a ] && echo 0.2.23-b || echo 0.2.23-a)
  expect=$([ "${to##*-}" = a ] && echo osodevops/kafka-backup:v0.19.0 || echo osodevops/kafka-backup:v0.19.1)
  old_pod=$(lease_holder); apply_before=$(cronjob_apply_time); tr_before=$(lease_transitions)
  PODLOG=$(watch_pods_bg "$EVID/$SCEN/pods-$i.jsonl"); LEASELOG=$(watch_lease_bg "$EVID/$SCEN/lease-$i.jsonl")
  ( for _ in $(seq 1 40); do touch_cr; sleep 0.5; done ) & TOUCH=$!
  operator_install "$CHART_FIX" "$to" "${SURGE[@]}"
  wait $TOUCH || true
  r=$(wait_for 30 img_is "$expect") || fail "run $i: image $(cronjob_image) != $expect"
  sleep 4; kill "$PODLOG" "$LEASELOG" 2>/dev/null || true
  new_pod=$(lease_holder)
  [ "$new_pod" != "$old_pod" ] && [ -n "$new_pod" ] || fail "run $i: lease holder did not change ($old_pod -> $new_pod)"
  summary=$(python3 "$ROOT/scripts/e2e/analyze.py" lease "$EVID/$SCEN/lease-$i.jsonl"); log "run $i lease: $summary"
  changes=$(echo "$summary" | python3 -c 'import sys,json; print(json.load(sys.stdin)["non_empty_holder_changes"])')
  handover=$(echo "$summary" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("handover_s"))')
  acquired=$(echo "$summary" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("acquired_at"))')
  [ "$changes" = 1 ] || fail "run $i: expected exactly one holder change, got $changes"
  [ "$(lease_transitions)" = "$((tr_before+1))" ] || fail "run $i: leaseTransitions $tr_before -> $(lease_transitions)"
  python3 -c "import sys; h=float('$handover'); sys.exit(0 if h <= 5 else 1)" || fail "run $i: handover took ${handover}s (> 5s)"
  [ "$(python3 "$ROOT/scripts/e2e/analyze.py" ready_before "$EVID/$SCEN/pods-$i.jsonl" "$new_pod" "$acquired")" = yes ] || fail "run $i: new pod was not Ready (standby) before it acquired the lease"
  apply_after=$(cronjob_apply_time)
  python3 - "$apply_before" "$apply_after" "$acquired" <<'PY' || fail "run $i: CronJob was applied by the new pod before it held the lease"
import sys,datetime
p=lambda s: datetime.datetime.fromisoformat(s.replace("Z","+00:00"))
before,after,acq=sys.argv[1:]
sys.exit(0 if after==before or p(after)>=p(acq)-datetime.timedelta(seconds=1) else 1)
PY
  python3 "$ROOT/scripts/e2e/analyze.py" overlap "$EVID/$SCEN/pods-$i.jsonl" | tee -a "$EVID/$SCEN/log.md" | grep -q "overlap=yes" || log "note: pods did not overlap in run $i (RollingUpdate usually overlaps)"
  evidence "run $i ($from -> $to): image ok ($r), holder $old_pod -> $new_pod, handover ${handover}s"
  pass "run $i"
  from=$to
done
