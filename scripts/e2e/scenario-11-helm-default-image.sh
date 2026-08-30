#!/usr/bin/env bash
# Scenario 11 — engine image precedence and the compatibility guard (issue #67):
# Helm backupJobs.image changes the CronJob image without a rebuild and shows up
# in the engine_image_info metric; spec.image wins over it; an engine below the
# minimum gets EngineVersionSupported=False (Job still built); unpinning and
# clearing the Helm value falls back to the compiled-in default.
export SCEN=11-helm-default-image; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cond_is() { [ "$(cr_condition incr EngineVersionSupported)" = "$1" ]; }
pull_policy() { k -n "$NS_KAFKA" get cronjob incr-scheduled -o jsonpath='{.spec.jobTemplate.spec.template.spec.containers[0].imagePullPolicy}'; }

operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b; apply_cr
wait_for 120 img_is "$(img_new)" >/dev/null || fail "precondition: CronJob on the compiled-in default"
r=$(wait_for 30 cond_is True/EngineVersionSupported) || fail "condition on the default: $(cr_condition incr EngineVersionSupported)"
[ -z "$(pull_policy)" ] || fail "imagePullPolicy set without backupJobs.imagePullPolicy: $(pull_policy)"
pod=$(op_pod_names | head -1)
metrics_of "$pod" | grep -q "strimzi_backup_operator_engine_image_info{image=\"$(img_new)\",source=\"compiled-in\"} 1" || fail "engine_image_info (compiled-in)"
evidence "compiled-in default $(img_new): $(cr_condition incr EngineVersionSupported) ($r)"
pass "compiled-in default"

# 1. installation-wide default via Helm, no operator rebuild
operator_install "$CHART_FIX" 0.2.23-b --set backupJobs.image="$(img_old)" --set backupJobs.imagePullPolicy=IfNotPresent
r=$(wait_for 60 img_is "$(img_old)") || fail "backupJobs.image not applied to the CronJob (image $(cronjob_image))"
[ "$(pull_policy)" = IfNotPresent ] || fail "imagePullPolicy $(pull_policy), expected IfNotPresent"
pod=$(op_pod_names | head -1)
metrics_of "$pod" | grep -q "strimzi_backup_operator_engine_image_info{image=\"$(img_old)\",source=\"env\"} 1" || fail "engine_image_info (env)"
evidence "backupJobs.image=$(img_old) ($r), pullPolicy=$(pull_policy)"
pass "backupJobs.image"

# 2. spec.image wins over the Helm default
k -n "$NS_KAFKA" patch kafkabackup incr --type=merge -p "{\"spec\":{\"image\":\"$(img_new)\"}}" >/dev/null
r=$(wait_for 30 img_is "$(img_new)") || fail "spec.image did not override backupJobs.image (image $(cronjob_image))"
evidence "spec.image=$(img_new) over backupJobs.image ($r)"
pass "spec.image precedence"

# 3. an engine below the minimum: CronJob still updated, condition False, counter bumped
k -n "$NS_KAFKA" patch kafkabackup incr --type=merge -p '{"spec":{"image":"osodevops/kafka-backup:v0.15.3"}}' >/dev/null
r=$(wait_for 30 img_is osodevops/kafka-backup:v0.15.3) || fail "old engine not applied (image $(cronjob_image))"
r2=$(wait_for 30 cond_is False/EngineOlderThanMinimum) || fail "condition: $(cr_condition incr EngineVersionSupported)"
metrics_of "$pod" | grep -Eq 'strimzi_backup_operator_engine_version_unsupported_total\{controller="backup"\} [1-9]' || fail "unsupported counter not incremented"
evidence "v0.15.3: $(cr_condition incr EngineVersionSupported) ($r, $r2)"
pass "EngineVersionSupported=False below the minimum"

# 4. unpin the resource and clear the Helm value: back to the compiled-in default
k -n "$NS_KAFKA" patch kafkabackup incr --type=json -p='[{"op":"remove","path":"/spec/image"}]' >/dev/null
operator_install "$CHART_FIX" 0.2.23-b
r=$(wait_for 60 img_is "$(img_new)") || fail "did not fall back to the compiled-in default (image $(cronjob_image))"
r2=$(wait_for 30 cond_is True/EngineVersionSupported) || fail "condition after fallback: $(cr_condition incr EngineVersionSupported)"
[ -z "$(pull_policy)" ] || fail "imagePullPolicy still set after clearing the value: $(pull_policy)"
evidence "fallback to $(img_new) ($r, $r2)"
pass "fallback to the compiled-in default"
