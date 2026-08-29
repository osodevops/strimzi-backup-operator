#!/usr/bin/env bash
# Scenario 7 — out-of-band CronJob edits are reverted via the CronJob watch
# (independent of leader election); negative control on 0.2.22.
export SCEN=07-oob-edit; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
patch_image() { k -n "$NS_KAFKA" patch cronjob incr-scheduled --type=json -p='[{"op":"replace","path":"/spec/jobTemplate/spec/template/spec/containers/0/image","value":"osodevops/kafka-backup:v0.19.0"}]' >/dev/null; }
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b --set leaderElection.enabled=false; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.1 >/dev/null || fail "precondition"
pod=$(op_pod_names | head -1); sleep 65   # let the +5s/+60s start-up resyncs pass
c0=$(reconcile_count "$pod")
patch_image; r=$(wait_for 10 img_is osodevops/kafka-backup:v0.19.1) || fail "image edit not reverted within 10s"
log "image edit reverted: $r"; cronjob_managers | tee -a "$EVID/$SCEN/log.md" >/dev/null
sleep 5; c1=$(reconcile_count "$pod"); sleep 30; c2=$(reconcile_count "$pod")
[ "$c1" -ge $((c0+1)) ] && [ "$c1" -le $((c0+3)) ] || fail "reconcile count moved $c0 -> $c1 (expected +1..+3)"
[ "$c2" = "$c1" ] || fail "reconcile storm: $c1 -> $c2 in 30s"
k -n "$NS_KAFKA" patch cronjob incr-scheduled --type=merge -p '{"spec":{"suspend":true}}' >/dev/null
r=$(wait_for 10 cronjob_suspend_is false) || fail "suspend edit not reverted"
log "suspend edit reverted: $r"
evidence "fixed build: edits reverted, reconciles $c0 -> $c1 -> $c2"
pass "out-of-band edits reverted, no storm"
# negative control
operator_uninstall; operator_install "$CHART_OLD" 0.2.22-b; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.1 >/dev/null || fail "precondition (old)"
patch_image; sleep 60; [ "$(cronjob_image)" = osodevops/kafka-backup:v0.19.0 ] || fail "negative control: 0.2.22 reverted the edit?"
evidence "0.2.22 negative control: edit still present after 60s"
pass "negative control on 0.2.22"
