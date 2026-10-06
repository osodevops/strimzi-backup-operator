#!/usr/bin/env bash
# Issue #76: backup storage served over HTTPS with a certificate signed by a
# private CA. Run against a baseline build and a fix build:
#   scripts/e2e/scenario-12-storage-private-ca.sh <operator-image-tag>
# Env: JOB_IMAGE (engine image for Job pods; default $ENGINE_NEW), CHART (default: this tree)
#
# Cases
#   1  no CA configured            -> Job fails (UnknownIssuer)      [both builds]
#   2  spec.storage.tls (one-shot) -> baseline: API rejects the field;
#                                     fix: Job backs up all records over HTTPS
#   3  scheduled + retention       -> fix: CronJob carries the CA, two runs, the
#                                     operator lists and prunes over HTTPS
#   4  KafkaRestore of case 2      -> fix: restore Job reads over HTTPS, all records
#   R  retention without the CA    -> baseline: the operator's own storage client
#                                     fails every reconcile (UnknownIssuer)
export SCEN="${SCEN:-12-storage-private-ca}"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
TAG="${1:?usage: $0 <operator-image-tag>}"
JOB_IMAGE="${JOB_IMAGE:-$ENGINE_NEW}"
CHART="${CHART:-$CHART_FIX}"   # chart (and so CRDs) to install; a baseline run passes the old tree's chart
TOPIC=issue76-orders
RECORDS=1000
PKI="$E2E_DIR/storage-ca"
KAFKA_IMAGE=quay.io/strimzi/kafka:0.46.1-kafka-4.0.0
OUT="$EVID/$SCEN/$TAG"; mkdir -p "$OUT"

# --- private CA + HTTPS MinIO -------------------------------------------------
if [ ! -f "$PKI/public.crt" ]; then
  mkdir -p "$PKI"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$PKI/ca.key" -out "$PKI/ca.crt" -days 30 \
    -subj "/O=strimzi-backup-operator e2e/CN=Issue76 Private Test CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$PKI/private.key" -out "$PKI/server.csr" -subj "/CN=minio-tls.minio.svc" 2>/dev/null
  printf '%s\n' "basicConstraints=critical,CA:FALSE" "keyUsage=critical,digitalSignature,keyEncipherment" \
    "extendedKeyUsage=serverAuth" \
    "subjectAltName=DNS:minio-tls,DNS:minio-tls.minio,DNS:minio-tls.minio.svc,DNS:minio-tls.minio.svc.cluster.local" > "$PKI/server.ext"
  openssl x509 -req -in "$PKI/server.csr" -CA "$PKI/ca.crt" -CAkey "$PKI/ca.key" -CAcreateserial \
    -out "$PKI/public.crt" -days 30 -extfile "$PKI/server.ext" 2>/dev/null
fi
k -n minio create secret generic minio-tls-certs --from-file=public.crt="$PKI/public.crt" \
  --from-file=private.key="$PKI/private.key" --from-file=ca.crt="$PKI/ca.crt" --dry-run=client -o yaml | k apply -f - >/dev/null
k -n "$NS_KAFKA" create secret generic minio-ca --from-file=ca.crt="$PKI/ca.crt" --dry-run=client -o yaml | k apply -f - >/dev/null
k apply -f "$ROOT/manifests/e2e/minio-tls.yaml" >/dev/null
k -n minio rollout status deploy/minio-tls --timeout=120s >/dev/null
k -n minio wait job/minio-tls-make-bucket --for=condition=complete --timeout=180s >/dev/null || fail "HTTPS MinIO bucket job"

# --- seed data ----------------------------------------------------------------
kafka_cli() { # <script>  — Kafka CLI as the SCRAM user, run inside the broker
  # (kubectl exec, not `kubectl run -i`, which can drop the output of a pod that exits quickly)
  local pw; pw=$(k -n "$NS_KAFKA" get secret kafka-backup -o jsonpath='{.data.password}' | base64 -d)
  k -n "$NS_KAFKA" exec -i my-cluster-dual-role-0 -c kafka -- sh -c "
printf '%s\n' 'security.protocol=SASL_PLAINTEXT' 'sasl.mechanism=SCRAM-SHA-512' \
  'sasl.jaas.config=org.apache.kafka.common.security.scram.ScramLoginModule required username=\"kafka-backup\" password=\"$pw\";' > /tmp/e2e-c.props
B=my-cluster-kafka-bootstrap:9092; K=/opt/kafka/bin
$1" 2>/dev/null; }
topic_records() { kafka_cli "\$K/kafka-get-offsets.sh --bootstrap-server \$B --command-config /tmp/e2e-c.props --topic $1" | awk -F: '{s+=$3} END {print s+0}'; }
if [ "$(topic_records $TOPIC)" -lt "$RECORDS" ]; then
  kafka_cli "\$K/kafka-topics.sh --bootstrap-server \$B --command-config /tmp/e2e-c.props --create --if-not-exists --topic $TOPIC --partitions 3 --replication-factor 1 >/dev/null
\$K/kafka-producer-perf-test.sh --topic $TOPIC --num-records $RECORDS --record-size 200 --throughput -1 --producer.config /tmp/e2e-c.props --producer-props bootstrap.servers=\$B acks=all | tail -1"
fi
log "topic $TOPIC holds $(topic_records $TOPIC) records"

# --- operator -------------------------------------------------------------------
for cr in issue76-no-ca issue76-trusted issue76-sched issue76-sched-no-ca; do
  k -n "$NS_KAFKA" delete kafkabackup "$cr" --ignore-not-found --wait=true >/dev/null
  k -n "$NS_KAFKA" delete jobs -l "kafkabackup.com/backup=$cr" --ignore-not-found --wait=true >/dev/null
done
k -n "$NS_KAFKA" delete kafkarestore issue76-restore --ignore-not-found --wait=true >/dev/null
k -n "$NS_KAFKA" delete jobs -l kafkabackup.com/restore=issue76-restore --ignore-not-found --wait=true >/dev/null
operator_uninstall
operator_install "$CHART" "$TAG" --set backupJobs.image="$JOB_IMAGE" --set backupJobs.imagePullPolicy=IfNotPresent
OP_POD=$(op_pod_names | head -n1)

backup_cr() { # <name> <prefix> <with-ca:yes|no> [extra spec yaml]
  cat <<EOF
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
      prefix: $2
      endpoint: https://minio-tls.minio.svc:9000
      forcePathStyle: true
      accessKeySecret: {name: minio-credentials, key: access-key-id}
      secretKeySecret: {name: minio-credentials, key: secret-access-key}
$([ "$3" = yes ] && printf '    tls:\n      trustedCertificates:\n        - {secretName: minio-ca, certificate: ca.crt}\n')
  backup: {stopAtCurrentOffsets: true}
  backoffLimit: 0
${4:-}
EOF
}
job_of() { k -n "$NS_KAFKA" get jobs -l "kafkabackup.com/backup=$1" -o jsonpath='{.items[0].metadata.name}' 2>/dev/null; }
job_done() { local s; s=$(k -n "$NS_KAFKA" get job "$1" -o jsonpath='{.status.succeeded}{.status.failed}' 2>/dev/null); [ -n "$s" ]; }
job_logs() { k -n "$NS_KAFKA" logs "job/$1" 2>&1 | sed 's/\x1b\[[0-9;]*m//g'; }
records_processed() { job_logs "$1" | awk '/^Records processed:/ {print $3}'; }
# wait_for re-runs its command, so conditions are functions, not $(...) snapshots.
has_job() { [ -n "$(job_of "$1")" ]; }
has_restore_job() { [ -n "$(k -n "$NS_KAFKA" get jobs -l kafkabackup.com/restore=issue76-restore -o jsonpath='{.items[0].metadata.name}' 2>/dev/null)" ]; }
last_backup_completed() { [ "$(k -n "$NS_KAFKA" get kafkabackup "$1" -o jsonpath='{.status.lastBackup.status}')" = Completed ]; }
# The reconcile error the retention pass returns once the client gives up retrying.
retention_storage_error() { op_logs "$OP_POD" | grep "Reconciliation error" | grep "issue76-sched-no-ca" | grep -q "Storage error"; }
touch_and_check_pruned() {
  k -n "$NS_KAFKA" annotate kafkabackup issue76-sched e2e/touch="$(date +%s%N)" --overwrite >/dev/null
  op_logs "$OP_POD" | grep "Pruned expired backup" | grep -q "$RUN1"
}
mc_list() { # <name> <shell>  — mc over HTTPS trusting only the private CA
  cat <<EOF | k apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata: {name: $1, namespace: minio}
spec:
  backoffLimit: 0
  template:
    spec:
      restartPolicy: Never
      containers:
        - name: mc
          image: quay.io/minio/mc:RELEASE.2025-08-13T08-35-41Z
          env: [{name: SSL_CERT_FILE, value: /ca/ca.crt}]
          command: ["sh", "-c", "mc alias set s https://minio-tls.minio.svc:9000 minioadmin minioadmin >/dev/null && $2"]
          volumeMounts: [{name: ca, mountPath: /ca}]
      volumes: [{name: ca, secret: {secretName: minio-tls-certs, items: [{key: ca.crt, path: ca.crt}]}}]
EOF
  k -n minio wait "job/$1" --for=condition=complete --timeout=120s >/dev/null
  k -n minio logs "job/$1"
  k -n minio delete job "$1" --wait=false >/dev/null
}

# --- case 1: no CA -> UnknownIssuer ------------------------------------------------
backup_cr issue76-no-ca issue76-no-ca no | k apply -f - >/dev/null
wait_for 60 has_job issue76-no-ca >/dev/null; J=$(job_of issue76-no-ca)
wait_for 180 job_done "$J" >/dev/null || fail "case 1: job $J did not finish"
job_logs "$J" > "$OUT/case1-no-ca-job.log"
grep -q "UnknownIssuer" "$OUT/case1-no-ca-job.log" && [ "$(k -n "$NS_KAFKA" get job "$J" -o jsonpath='{.status.failed}')" = 1 ] \
  && pass "case 1: without a trusted CA the backup Job fails: $(grep -m1 -o 'invalid peer certificate: UnknownIssuer' "$OUT/case1-no-ca-job.log")" \
  || fail "case 1: expected an UnknownIssuer failure"
k -n "$NS_KAFKA" delete kafkabackup issue76-no-ca --wait=false >/dev/null

# --- case 2: one-shot with spec.storage.tls -----------------------------------------
if ! backup_cr issue76-trusted issue76-trusted yes | k apply -f - > "$OUT/case2-apply.log" 2>&1; then
  log "case 2: API rejected spec.storage.tls: $(cat "$OUT/case2-apply.log")"
  pass "case 2 (baseline): there is no supported way to supply the CA"
  BASELINE=1
else
  BASELINE=0
  wait_for 60 has_job issue76-trusted >/dev/null; J=$(job_of issue76-trusted)
  k -n "$NS_KAFKA" get job "$J" -o json | python3 -c '
import json, sys
p = json.load(sys.stdin)["spec"]["template"]["spec"]; c = p["containers"][0]
print("volume:", json.dumps([v for v in p["volumes"] if v["name"] == "storage-trusted-certs"]))
print("mount: ", json.dumps([m for m in c["volumeMounts"] if m["name"] == "storage-trusted-certs"]))
print("env:   ", json.dumps([e for e in c["env"] if e["name"] == "SSL_CERT_DIR"]))' | tee "$OUT/case2-job-pod-spec.txt" >&2
  wait_for 180 job_done "$J" >/dev/null || fail "case 2: job $J did not finish"
  job_logs "$J" > "$OUT/case2-trusted-job.log"
  got=$(records_processed "$J")
  [ "$(k -n "$NS_KAFKA" get job "$J" -o jsonpath='{.status.succeeded}')" = 1 ] && [ "$got" = "$RECORDS" ] \
    && ! grep -q UnknownIssuer "$OUT/case2-trusted-job.log" \
    && pass "case 2: backup Job $J completed over HTTPS, Records processed: $got" \
    || fail "case 2: expected $RECORDS records, got '${got}'"
  wait_for 60 last_backup_completed issue76-trusted >/dev/null \
    || fail "case 2: status.lastBackup not Completed"
  k -n "$NS_KAFKA" get kafkabackup issue76-trusted -o jsonpath='{range .status.conditions[*]}{.type}={.status} {.reason}{"\n"}{end}' | tee "$OUT/case2-conditions.txt" >&2
  BACKUP_ID=$(k -n "$NS_KAFKA" get kafkabackup issue76-trusted -o jsonpath='{.status.lastBackup.id}')
fi

# --- case 3: scheduled + retention over HTTPS (fix) / without CA (both) -----------
SCHED='  schedule: {cron: "0 0 31 2 *"}
  retention: {maxBackups: 1, pruneOnSchedule: true}'
if [ "$BASELINE" = 1 ]; then
  backup_cr issue76-sched-no-ca issue76-sched-no-ca no "$SCHED" | k apply -f - >/dev/null
  wait_for 120 retention_storage_error >/dev/null || true
  op_logs "$OP_POD" | grep "Reconciliation error" | grep "issue76-sched-no-ca" | tail -n 3 > "$OUT/caseR-operator-retention.log"
  [ -s "$OUT/caseR-operator-retention.log" ] \
    && pass "case R (baseline): every reconcile fails in the operator's retention pass: $(grep -m1 -o 'Storage error[^"]\{0,150\}' "$OUT/caseR-operator-retention.log")" \
    || fail "case R: expected a retention Storage error (see $OUT/caseR-operator-retention.log)"
  k -n "$NS_KAFKA" delete kafkabackup issue76-sched-no-ca --wait=false >/dev/null
  exit 0
fi

backup_cr issue76-sched issue76-sched yes "$SCHED" | k apply -f - >/dev/null
wait_for 60 k -n "$NS_KAFKA" get cronjob issue76-sched-scheduled >/dev/null || fail "case 3: CronJob not created"
k -n "$NS_KAFKA" get cronjob issue76-sched-scheduled -o json | python3 -c '
import json, sys
p = json.load(sys.stdin)["spec"]["jobTemplate"]["spec"]["template"]["spec"]; c = p["containers"][0]
print("volume:", json.dumps([v for v in p["volumes"] if v["name"] == "storage-trusted-certs"]))
print("env:   ", json.dumps([e for e in c["env"] if e["name"] == "SSL_CERT_DIR"]))' | tee "$OUT/case3-cronjob-pod-spec.txt" >&2
for run in 1 2; do
  J="issue76-sched-run$run-$(date +%s)"
  k -n "$NS_KAFKA" create job --from=cronjob/issue76-sched-scheduled "$J" >/dev/null
  wait_for 180 job_done "$J" >/dev/null || fail "case 3: run $run did not finish"
  job_logs "$J" > "$OUT/case3-run$run.log"
  [ "$(k -n "$NS_KAFKA" get job "$J" -o jsonpath='{.status.succeeded}')" = 1 ] || fail "case 3: run $run failed"
  log "case 3: scheduled run $J completed, Records processed: $(records_processed "$J")"
  eval "RUN$run=$J"
done
wait_for 120 touch_and_check_pruned >/dev/null \
  || fail "case 3: the operator did not prune $RUN1 over HTTPS"
op_logs "$OP_POD" | grep -E "Pruned expired backup|Applied backup retention policy|Storage error|UnknownIssuer" | sed 's/\x1b\[[0-9;]*m//g' | tail -n 6 > "$OUT/case3-operator-retention.log"
grep -q "UnknownIssuer\|Storage error" "$OUT/case3-operator-retention.log" && fail "case 3: storage errors in operator log"
history=$(k -n "$NS_KAFKA" get kafkabackup issue76-sched -o jsonpath='{range .status.backupHistory[*]}{.id}{" "}{end}')
log "case 3: status.backupHistory after retention = [$history]"
mc_list "issue76-mc-ls-$(date +%s)" "mc ls s/kafka-backups/issue76-sched/ && echo --- && mc ls --recursive s/kafka-backups/issue76-trusted/" > "$OUT/case3-bucket-listing.txt"
cat "$OUT/case3-bucket-listing.txt" >&2
grep -q "$RUN2/" "$OUT/case3-bucket-listing.txt" && ! grep -q "$RUN1/" "$OUT/case3-bucket-listing.txt" \
  && pass "case 3: retention (maxBackups=1) listed and pruned $RUN1 over HTTPS; $RUN2 kept" \
  || fail "case 3: unexpected bucket contents"

# --- case 4: restore over HTTPS ---------------------------------------------------
# A fresh target per run, with the source's partition count (auto-creation would give 1).
RESTORED="$TOPIC-restored-$(date +%s)"
kafka_cli "\$K/kafka-topics.sh --bootstrap-server \$B --command-config /tmp/e2e-c.props --create --topic $RESTORED --partitions 3 --replication-factor 1" >/dev/null
cat <<EOF | k apply -f - >/dev/null
apiVersion: kafkabackup.com/v1
kind: KafkaRestore
metadata: {name: issue76-restore, namespace: $NS_KAFKA}
spec:
  strimziClusterRef: {name: my-cluster}
  authentication: {type: scram-sha-512, kafkaUserRef: {name: kafka-backup}}
  backupRef: {name: issue76-trusted, backupId: "$BACKUP_ID"}
  topics: {include: ["$TOPIC"]}
  topicMapping:
    - {sourceTopic: $TOPIC, targetTopic: $RESTORED}
EOF
wait_for 60 has_restore_job >/dev/null
RJ=$(k -n "$NS_KAFKA" get jobs -l kafkabackup.com/restore=issue76-restore -o jsonpath='{.items[0].metadata.name}')
wait_for 240 job_done "$RJ" >/dev/null || fail "case 4: restore job $RJ did not finish"
job_logs "$RJ" > "$OUT/case4-restore-job.log"
[ "$(k -n "$NS_KAFKA" get job "$RJ" -o jsonpath='{.status.succeeded}')" = 1 ] || fail "case 4: restore job failed"
restored=$(topic_records "$RESTORED")
[ "$restored" = "$RECORDS" ] && pass "case 4: restore Job $RJ read the backup over HTTPS; $RESTORED holds $restored records" \
  || fail "case 4: expected $RECORDS restored records, got $restored"
