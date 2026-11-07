#!/usr/bin/env bash
# Issues #79/#80: exercise the real operator, Kafka and MinIO together.
# Usage: scenario-13-engine-error-metrics.sh <operator-image-tag>
# OPERATOR_REPO may select a local build; JOB_IMAGE overrides the engine for a
# source-build run. Leave both unset to verify the published release defaults.
# CHART may select an unpacked published chart instead of the checkout.
# EXPECT_ZERO_ERRORS=false reproduces the missing series on an older engine.
export SCEN="${SCEN:-13-engine-error-metrics}"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
TAG="${1:?usage: $0 <operator-image-tag>}"
CHART="${CHART:-$CHART_FIX}"
EXPECTED_IMAGE="${JOB_IMAGE:-$ENGINE_NEW}"
EXPECT_ZERO_ERRORS="${EXPECT_ZERO_ERRORS:-true}"
OUT="$EVID/$SCEN/$TAG"; mkdir -p "$OUT"
TOPIC=issue7980-orders
RECORDS=1000

kafka_cli() {
  local pw; pw=$(k -n "$NS_KAFKA" get secret kafka-backup -o jsonpath='{.data.password}' | base64 -d)
  k -n "$NS_KAFKA" exec -i my-cluster-dual-role-0 -c kafka -- sh -c "
printf '%s\n' 'security.protocol=SASL_PLAINTEXT' 'sasl.mechanism=SCRAM-SHA-512' \
  'sasl.jaas.config=org.apache.kafka.common.security.scram.ScramLoginModule required username=\"kafka-backup\" password=\"$pw\";' > /tmp/issue7980.props
B=my-cluster-kafka-bootstrap:9092; K=/opt/kafka/bin
$1" 2>/dev/null
}
topic_records() {
  kafka_cli "\$K/kafka-get-offsets.sh --bootstrap-server \$B --command-config /tmp/issue7980.props --topic $1" | awk -F: '{s+=$3} END {print s+0}'
}
if [ "$(topic_records "$TOPIC")" -eq 0 ]; then
  kafka_cli "\$K/kafka-topics.sh --bootstrap-server \$B --command-config /tmp/issue7980.props --create --if-not-exists --topic $TOPIC --partitions 3 --replication-factor 1 >/dev/null
\$K/kafka-producer-perf-test.sh --topic $TOPIC --num-records $RECORDS --record-size 200 --throughput -1 --producer.config /tmp/issue7980.props --producer-props bootstrap.servers=\$B acks=all | tail -1"
fi
[ "$(topic_records "$TOPIC")" = "$RECORDS" ] || fail "seed topic must have exactly $RECORDS records"

# Preserve unrelated resources and install the requested operator artifact.
for name in issue7980-snapshot issue7980-scheduled; do
  k -n "$NS_KAFKA" delete kafkabackup "$name" --ignore-not-found --wait=true >/dev/null
done
k -n "$NS_KAFKA" delete kafkarestore issue7980-restore --ignore-not-found --wait=true >/dev/null
h upgrade --install "$RELEASE" "$CHART" -n "$NS_OP" --create-namespace \
  --set image.repository="${OPERATOR_REPO:-ghcr.io/osodevops/strimzi-backup-operator}" \
  --set image.tag="$TAG" --set image.pullPolicy=IfNotPresent \
  --set backupJobs.image="${JOB_IMAGE:-}" --set backupJobs.imagePullPolicy=IfNotPresent \
  --wait --timeout 180s > "$OUT/helm.log"
OP_POD=$(op_pod_names | head -1)
metrics_of "$OP_POD" > "$OUT/operator.prom"
grep -F "image=\"$EXPECTED_IMAGE\"" "$OUT/operator.prom" >/dev/null || fail "operator default image metric"

backup_cr() {
  cat <<YAML
apiVersion: kafkabackup.com/v1
kind: KafkaBackup
metadata: {name: $1, namespace: $NS_KAFKA}
spec:
  strimziClusterRef: {name: my-cluster}
  authentication: {type: scram-sha-512, kafkaUserRef: {name: kafka-backup}}
  topics: {include: ["$TOPIC"]}
  storage:
    type: s3
    s3:
      bucket: kafka-backups
      region: us-east-1
      prefix: $1
      endpoint: http://minio.minio.svc.cluster.local:9000
      forcePathStyle: true
      allowHttp: true
      accessKeySecret: {name: minio-credentials, key: access-key-id}
      secretKeySecret: {name: minio-credentials, key: secret-access-key}
  backup: {stopAtCurrentOffsets: true}
  metrics: {enabled: true, keepAliveSeconds: 30}
  backoffLimit: 0
${2:-}
YAML
}
job_of() { k -n "$NS_KAFKA" get jobs -l "kafkabackup.com/backup=$1" -o jsonpath='{.items[0].metadata.name}' 2>/dev/null; }
has_job() { [ -n "$(job_of "$1")" ]; }
last_backup_completed() { [ "$(k -n "$NS_KAFKA" get kafkabackup "$1" -o jsonpath='{.status.lastBackup.status}')" = Completed ]; }
assert_image() {
  local got; got=$(k -n "$NS_KAFKA" get job "$1" -o jsonpath='{.spec.template.spec.containers[0].image}')
  [ "$got" = "$EXPECTED_IMAGE" ] || fail "$1 image=$got, expected $EXPECTED_IMAGE"
}
backup_metrics_ready() {
  local pod; pod=$(k -n "$NS_KAFKA" get pods -l job-name="$1" -o jsonpath='{.items[0].metadata.name}' 2>/dev/null)
  k get --raw "/api/v1/namespaces/$NS_KAFKA/pods/$pod:8080/proxy/metrics" > "$OUT/$1.prom" 2>/dev/null || return 1
  python3 - "$OUT/$1.prom" "$RECORDS" <<'PY'
import sys
text = open(sys.argv[1]).read()
values = [float(line.split()[-1]) for line in text.splitlines() if line.startswith('kafka_backup_records_total{')]
sys.exit(0 if sum(values) == int(sys.argv[2]) else 1)
PY
}
verify_backup() {
  assert_image "$1"
  wait_for 180 backup_metrics_ready "$1" >/dev/null || fail "$1: could not scrape $RECORDS backed-up records"
  python3 - "$OUT/$1.prom" "$EXPECT_ZERO_ERRORS" <<'PY'
import sys
text = open(sys.argv[1]).read()
samples = [l for l in text.splitlines() if l.startswith('kafka_backup_errors_total{')]
if sys.argv[2] == 'true':
    assert len(samples) == 11, samples
    assert all(float(l.split()[-1]) == 0 for l in samples), samples
    assert all('backup_id="' in l and 'error_type="' in l for l in samples)
else:
    assert not samples, samples
assert '_total_total' not in text
print('backup error samples:', len(samples))
PY
  k -n "$NS_KAFKA" wait job/"$1" --for=condition=complete --timeout=180s >/dev/null || fail "$1 did not succeed"
  k -n "$NS_KAFKA" logs job/"$1" > "$OUT/$1.log"
  ! grep -q 'Ignoring unknown config key' "$OUT/$1.log" || fail "$1: unknown engine config key"
  pass "$1: image $EXPECTED_IMAGE, $RECORDS records, expected error samples, successful Job"
}

backup_cr issue7980-snapshot | k apply -f - >/dev/null
wait_for 60 has_job issue7980-snapshot >/dev/null || fail "one-shot Job was not created"
SNAPSHOT_JOB=$(job_of issue7980-snapshot)
verify_backup "$SNAPSHOT_JOB"
wait_for 60 last_backup_completed issue7980-snapshot >/dev/null || fail "operator did not record Completed status"
BACKUP_ID=$(k -n "$NS_KAFKA" get kafkabackup issue7980-snapshot -o jsonpath='{.status.lastBackup.id}')

backup_cr issue7980-scheduled '  schedule: {cron: "0 0 31 2 *"}' | k apply -f - >/dev/null
wait_for 60 k -n "$NS_KAFKA" get cronjob issue7980-scheduled-scheduled >/dev/null || fail "CronJob missing"
SCHEDULE_JOB="issue7980-run-$(date +%s)"
k -n "$NS_KAFKA" create job --from=cronjob/issue7980-scheduled-scheduled "$SCHEDULE_JOB" >/dev/null
verify_backup "$SCHEDULE_JOB"

RESTORED="$TOPIC-restored-$(date +%s)"
kafka_cli "\$K/kafka-topics.sh --bootstrap-server \$B --command-config /tmp/issue7980.props --create --topic $RESTORED --partitions 3 --replication-factor 1" >/dev/null
cat <<YAML | k apply -f - >/dev/null
apiVersion: kafkabackup.com/v1
kind: KafkaRestore
metadata: {name: issue7980-restore, namespace: $NS_KAFKA}
spec:
  strimziClusterRef: {name: my-cluster}
  authentication: {type: scram-sha-512, kafkaUserRef: {name: kafka-backup}}
  backupRef: {name: issue7980-snapshot, backupId: "$BACKUP_ID"}
  topics: {include: ["$TOPIC"]}
  topicMapping:
    - {sourceTopic: $TOPIC, targetTopic: $RESTORED}
YAML
restore_job() { k -n "$NS_KAFKA" get jobs -l kafkabackup.com/restore=issue7980-restore -o jsonpath='{.items[0].metadata.name}' 2>/dev/null; }
has_restore_job() { [ -n "$(restore_job)" ]; }
wait_for 60 has_restore_job >/dev/null || fail "restore Job missing"
RESTORE_JOB=$(restore_job)
assert_image "$RESTORE_JOB"
k -n "$NS_KAFKA" wait job/"$RESTORE_JOB" --for=condition=complete --timeout=240s >/dev/null || fail "restore Job failed"
k -n "$NS_KAFKA" logs job/"$RESTORE_JOB" > "$OUT/$RESTORE_JOB.log"
[ "$(topic_records "$RESTORED")" = "$RECORDS" ] || fail "restore record count mismatch"
k -n "$NS_KAFKA" get kafkabackup issue7980-snapshot -o yaml > "$OUT/backup-status.yaml"
k -n "$NS_KAFKA" get kafkarestore issue7980-restore -o yaml > "$OUT/restore-status.yaml"
pass "restore: image $EXPECTED_IMAGE, $RECORDS records restored; evidence: $OUT"
