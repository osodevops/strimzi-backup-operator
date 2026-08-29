#!/usr/bin/env bash
# Bring up the dedicated minikube profile with Strimzi, a single-node KRaft
# Kafka, a KafkaUser and MinIO. Idempotent.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mkdir -p "$E2E_DIR"
[ -f "$E2E_DIR/prev-context" ] || kubectl config current-context > "$E2E_DIR/prev-context" 2>/dev/null || true
if ! minikube status -p "$PROFILE" >/dev/null 2>&1; then
  minikube start -p "$PROFILE" --driver=docker --cpus=4 --memory=8192 --kubernetes-version=v1.33.1
  kubectl config use-context "$(cat "$E2E_DIR/prev-context")" >/dev/null 2>&1 || true
fi
helm repo add strimzi https://strimzi.io/charts/ >/dev/null 2>&1 || true
helm repo update strimzi >/dev/null
h upgrade --install strimzi strimzi/strimzi-kafka-operator --version 0.46.1 -n "$NS_KAFKA" --create-namespace \
  --set "watchNamespaces={$NS_KAFKA}" --wait --timeout 5m >/dev/null
k apply -f "$ROOT/manifests/e2e/kafka.yaml" >/dev/null
k apply -f "$ROOT/manifests/e2e/minio.yaml" >/dev/null
k -n "$NS_KAFKA" create secret generic minio-credentials --from-literal=access-key-id=minioadmin --from-literal=secret-access-key=minioadmin --dry-run=client -o yaml | k apply -f - >/dev/null
k -n "$NS_KAFKA" wait kafka/my-cluster --for=condition=Ready --timeout=10m
k -n "$NS_KAFKA" wait kafkauser/kafka-backup --for=condition=Ready --timeout=3m
k -n minio wait job/minio-make-bucket --for=condition=complete --timeout=5m
k -n "$NS_OP" get ns >/dev/null 2>&1 || k create ns "$NS_OP" >/dev/null
# Job pods run under a ServiceAccount named after the operator in the CR namespace.
k -n "$NS_KAFKA" get sa strimzi-backup-operator >/dev/null 2>&1 || k -n "$NS_KAFKA" create sa strimzi-backup-operator >/dev/null
echo "stack ready on profile $PROFILE"
