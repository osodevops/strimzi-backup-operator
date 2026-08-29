#!/usr/bin/env bash
# Prove the stack works end to end once: a manual run of the scheduled backup
# must complete (Kafka auth, MinIO, operator wiring).
export SCEN=00-smoke; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b; apply_cr
wait_for 120 img_is osodevops/kafka-backup:v0.19.1 >/dev/null || fail "CronJob not created"
job="incr-smoke-$(date +%s)"
k -n "$NS_KAFKA" create job --from=cronjob/incr-scheduled "$job" >/dev/null
k -n "$NS_KAFKA" wait job/"$job" --for=condition=complete --timeout=10m >/dev/null || { k -n "$NS_KAFKA" logs -l job-name="$job" --tail=40 | tee -a "$EVID/$SCEN/log.md"; fail "smoke backup job did not complete"; }
k -n "$NS_KAFKA" logs -l job-name="$job" --tail=200 | grep -E "Records processed|completed successfully|Errors:" | tee -a "$EVID/$SCEN/log.md"
k -n "$NS_KAFKA" delete job "$job" --wait=false >/dev/null
evidence "smoke ok (job $job)"; pass "stack smoke"
