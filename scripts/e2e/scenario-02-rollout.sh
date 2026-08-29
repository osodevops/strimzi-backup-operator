#!/usr/bin/env bash
# Scenario 2 — fixed chart, default rollout (maxSurge 0 / maxUnavailable 1):
# the real upgrade path (0.2.22 chart -> fix chart, where the old pod does not
# take part in the election) and fixed -> fixed, each with the touch loop.
# Then the opt-in Recreate strategy on a fresh install: no pod overlap at all.
export SCEN=02-rollout; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
N=${N:-2}
rolling_spec() { k -n "$NS_OP" get deploy "$RELEASE" -o jsonpath='{.spec.strategy.type}/{.spec.strategy.rollingUpdate.maxSurge}/{.spec.strategy.rollingUpdate.maxUnavailable}'; }
upgrade_and_check() { # <to-tag> <chart> <label> <expect-overlap yes|no|any> [helm args...]
  local to=$1 chart=$2 label=$3 overlap=$4; shift 4
  local expect; expect=$([ "${to##*-}" = a ] && echo osodevops/kafka-backup:v0.19.0 || echo osodevops/kafka-backup:v0.19.1)
  PODLOG=$(watch_pods_bg "$EVID/$SCEN/pods-$label.jsonl")
  ( for _ in $(seq 1 40); do touch_cr; sleep 0.5; done ) & TOUCH=$!
  local t0; t0=$(date +%s)
  operator_install "$chart" "$to" "$@"
  wait $TOUCH || true
  local r; r=$(wait_for 30 img_is "$expect") || { kill "$PODLOG" 2>/dev/null; evidence "FAILED $label"; fail "$label: CronJob image $(cronjob_image), expected $expect within 30s of helm returning"; }
  sleep 3; kill "$PODLOG" 2>/dev/null || true
  local ov; ov=$(python3 "$ROOT/scripts/e2e/analyze.py" overlap "$EVID/$SCEN/pods-$label.jsonl" | tee -a "$EVID/$SCEN/log.md" | grep -o "overlap=.*")
  [ "$overlap" = any ] || [ "$ov" = "overlap=$overlap" ] || fail "$label: $ov, expected overlap=$overlap"
  local apply1; apply1=$(cronjob_apply_time); sleep 20
  [ "$(cronjob_apply_time)" = "$apply1" ] && [ "$(cronjob_image)" = "$expect" ] || fail "$label: CronJob changed again after the rollout"
  evidence "$label (-> $to, strategy $(rolling_spec)): image correct ($r after helm, total $(( $(date +%s) - t0 ))s), $ov"
  pass "$label"
}
operator_uninstall
operator_install "$CHART_OLD" 0.2.22-a; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.0 >/dev/null || fail "precondition"
upgrade_and_check 0.2.23-b "$CHART_FIX" real-upgrade any
[ "$(rolling_spec)" = "RollingUpdate/0/1" ] || fail "default strategy is $(rolling_spec), expected RollingUpdate/0/1"
for i in $(seq 1 "$N"); do
  upgrade_and_check 0.2.23-a "$CHART_FIX" "fixed-b-to-a-$i" any
  upgrade_and_check 0.2.23-b "$CHART_FIX" "fixed-a-to-b-$i" any
done
# Opt-in Recreate on a fresh install: the old pod is fully gone before the new one starts.
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-a --set updateStrategy.type=Recreate; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.0 >/dev/null || fail "precondition (recreate)"
[ "$(strategy_type)" = Recreate ] || fail "strategy $(strategy_type)"
upgrade_and_check 0.2.23-b "$CHART_FIX" recreate-a-to-b no --set updateStrategy.type=Recreate
upgrade_and_check 0.2.23-a "$CHART_FIX" recreate-b-to-a no --set updateStrategy.type=Recreate
