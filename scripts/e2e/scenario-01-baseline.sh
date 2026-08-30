#!/usr/bin/env bash
# Scenario 1 — reproduce #62 on the unfixed 0.2.22 chart/binary (RollingUpdate,
# no leader election). Amplified: preStop sleep keeps the old pod alive after
# the new pod is Ready, a touch loop keeps both pods reconciling.
export SCEN=01-baseline; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
N=${N:-3}
img_of() { case "$1" in a|*-a) img_old;; b|*-b) img_new;; esac; }

operator_uninstall
operator_install "$CHART_OLD" 0.2.22-a
apply_cr
wait_for 120 img_is "$(img_of a)" >/dev/null || fail "CronJob never reached $(img_old)"
[ "$(strategy_type)" = "RollingUpdate" ] || fail "expected RollingUpdate on the old chart, got $(strategy_type)"
k -n "$NS_OP" patch deploy "$RELEASE" --type=json -p='[{"op":"add","path":"/spec/template/spec/containers/0/lifecycle","value":{"preStop":{"exec":{"command":["sh","-c","sleep 20"]}}}}]' >/dev/null
k -n "$NS_OP" rollout status deploy/"$RELEASE" --timeout=120s >/dev/null
evidence "start (0.2.22-a, preStop sleep 20)"

stale=0
from=0.2.22-a
for i in $(seq 1 "$N"); do
  to=$([ "$from" = 0.2.22-a ] && echo 0.2.22-b || echo 0.2.22-a)
  PODLOG=$(watch_pods_bg "$EVID/$SCEN/pods-$i.jsonl")
  ( for _ in $(seq 1 40); do touch_cr; sleep 0.5; done ) &
  TOUCH=$!
  h upgrade "$RELEASE" "$CHART_OLD" -n "$NS_OP" --reuse-values --set image.tag="$to" --wait --timeout 3m >/dev/null
  wait $TOUCH || true
  sleep 3; kill "$PODLOG" 2>/dev/null || true
  img=$(cronjob_image); apply=$(cronjob_apply_time)
  python3 "$ROOT/scripts/e2e/analyze.py" overlap "$EVID/$SCEN/pods-$i.jsonl" | tee -a "$EVID/$SCEN/log.md"
  if [ "$img" = "$(img_of "$from")" ]; then
    stale=$((stale+1)); log "run $i ($from -> $to): STALE image=$img lastApply=$apply"
    if wait_for 120 img_is "$(img_of "$to")" >/dev/null; then log "run $i: healed on its own within 120s (unexpected but not a failure of the repro)"; else
      log "run $i: still stale after 120s (last apply unchanged: $(cronjob_apply_time))"
      touch_cr; r=$(wait_for 15 img_is "$(img_of "$to")") || fail "touching the CR must heal the CronJob"; log "run $i: healed after CR touch: $r"
    fi
  else
    log "run $i ($from -> $to): fresh image=$img lastApply=$apply"
  fi
  evidence "after run $i"
  from=$to
done
log "baseline result: stale in $stale of $N amplified upgrades"
[ "$stale" -ge 1 ] && pass "reproduced #62 ($stale/$N)" || log "WARN: could not reproduce the race in $N runs"
