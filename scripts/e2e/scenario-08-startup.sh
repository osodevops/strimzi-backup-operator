#!/usr/bin/env bash
# Scenario 8 — a stale CronJob is corrected right after operator start-up, and
# the +5s resync tick fires without any CR event.
export SCEN=08-startup; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.1 >/dev/null || fail "precondition"
k -n "$NS_OP" scale deploy "$RELEASE" --replicas=0 >/dev/null
k -n "$NS_OP" wait pod -l app.kubernetes.io/name=strimzi-backup-operator --for=delete --timeout=60s >/dev/null 2>&1 || true
k -n "$NS_KAFKA" patch cronjob incr-scheduled --type=json -p='[{"op":"replace","path":"/spec/jobTemplate/spec/template/spec/containers/0/image","value":"osodevops/kafka-backup:v0.19.0"}]' >/dev/null
k -n "$NS_OP" scale deploy "$RELEASE" --replicas=1 >/dev/null
r=$(wait_for 120 img_is osodevops/kafka-backup:v0.19.1) || fail "stale CronJob not corrected after restart"
wait_for 30 has_holder >/dev/null || fail "no leader after restart"; pod=$(lease_holder)
started_of() { pod_times | grep "^$pod" | cut -f3 | cut -d= -f2; }
started_known() { [ "$(started_of)" != "-" ]; }
wait_for 30 started_known >/dev/null || fail "pod $pod has no start time"; started=$(started_of)
apply=$(cronjob_apply_time)
delta=$(python3 -c "import datetime as d; p=lambda s: d.datetime.fromisoformat(s.replace('Z','+00:00')); print(round((p('$apply')-p('$started')).total_seconds(),1))")
python3 -c "import sys; sys.exit(0 if float('$delta') <= 15 else 1)" || fail "correction ${delta}s after container start (> 15s)"
sleep 70
ticks=$(op_logs "$pod" | grep -c "Post-startup reconcile of all resources"); [ "$ticks" -ge 2 ] || fail "expected 2 start-up resync ticks, saw $ticks"
recs=$(op_logs "$pod" | grep -c "Reconciling KafkaBackup"); [ "$recs" -ge 3 ] || fail "expected initial + 2 resync reconciles, saw $recs"
evidence "start-up: corrected ${delta}s after start ($r), ticks=$ticks reconciles=$recs"
pass "start-up correction + resync ticks"
