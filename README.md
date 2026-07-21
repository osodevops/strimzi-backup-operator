# Kafka Backup Operator for Strimzi

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.27%2B-326ce5.svg)](https://kubernetes.io)

> **Disclaimer:** This project is not part of the [Strimzi](https://strimzi.io/) project or the CNCF. It is an independent, community-built operator designed to work with Strimzi-managed Kafka clusters.

A Kubernetes operator for **Kafka backup** and disaster recovery of Strimzi-managed Apache Kafka clusters. Provides dedicated CRDs for automated Kafka backup scheduling, point-in-time recovery, and multi-cloud storage — designed for the Strimzi ecosystem.

## Why Kafka Backup?

Strimzi makes running Apache Kafka on Kubernetes straightforward, but **backup and disaster recovery remain unsolved problems** in the Strimzi ecosystem:

- **MirrorMaker2 is not a backup** — it requires a full secondary cluster, expensive cross-cluster replication, and complex client failover procedures.
- **PVC snapshots are fragile** — deleting Strimzi CRDs triggers garbage collection of PVCs, and node failures can result in permanent data loss.
- **No point-in-time recovery** — there is no native mechanism to restore a Strimzi Kafka cluster to a specific moment in time.
- **No Strimzi-compatible backup CRD** — backup workflows are entirely manual or require external tools that don't integrate with the Strimzi operator model.

The **Kafka Backup Operator** solves these problems with a purpose-built Kubernetes operator that follows Strimzi conventions and is designed to work with Strimzi-managed clusters, providing first-class backup and restore capabilities.

## Features

- **Strimzi-compatible CRDs** — `KafkaBackup` and `KafkaRestore` custom resources under the `kafkabackup.com` API group, following Strimzi conventions for status conditions, labels, and finalizers
- **Auto-discovery of Strimzi resources** — automatically resolves bootstrap servers, TLS certificates, and KafkaUser credentials from your existing Strimzi `Kafka` CRs
- **Scheduled backups** — cron-based scheduling with timezone support via Kubernetes CronJobs
- **Point-in-time recovery (PITR)** — restore your Kafka cluster to any millisecond-precision timestamp
- **Multi-cloud storage** — back up to Amazon S3, Azure Blob Storage, Google Cloud Storage, or any S3-compatible store (MinIO, Ceph RGW)
- **Topic filtering** — include/exclude topics using glob or regex patterns, for both backup and restore
- **Topic mapping** — rename topics during restore for migration or testing scenarios
- **Consumer group offset restore** — restore consumer group offsets with optional group remapping
- **Retention policies** — automatic pruning of old backups by count or age
- **Compression** — gzip, snappy, lz4, or zstd compression for storage efficiency
- **Prometheus metrics** — built-in observability with backup/restore counters, duration histograms, and storage gauges
- **Pod template customisation** — full control over backup/restore Job pods (affinity, tolerations, host aliases, service account, security context, environment variables)
- **Azure Workload Identity** — native support for passwordless Azure authentication

## Architecture

The Kafka Backup Operator creates Kubernetes Jobs (or CronJobs for scheduled backups) that run the `kafka-backup` CLI to perform backup and restore operations. This provides resource isolation, failure isolation, and pod-level customisation.

```
+------------------------------------------------------------+
|                    Kubernetes Cluster                       |
|                                                            |
|  +------------------+         +------------------------+   |
|  |  Strimzi         |         |  Kafka Backup          |   |
|  |  Cluster         | <------ |  Operator              |   |
|  |  Operator        |  reads  |  (watches KafkaBackup  |   |
|  |                  |  Kafka  |   & KafkaRestore CRs)  |   |
|  +--------+---------+   CRs  +-----------+------------+   |
|           |                               |                |
|           v                               v creates        |
|  +------------------+         +------------------------+   |
|  |  Kafka CR        |         |  Backup/Restore Jobs   |   |
|  |  + Brokers       | <------ |  (kafka-backup CLI)    |   |
|  |  + ZooKeeper     |  reads  |                        |   |
|  +------------------+  data   +-----------+------------+   |
|                                           |                |
|                                           v writes/reads   |
|                               +------------------------+   |
|                               |  Object Storage        |   |
|                               |  (S3 / Azure / GCS)    |   |
|                               +------------------------+   |
+------------------------------------------------------------+
```

## Quick Start

### Prerequisites

- Kubernetes 1.27+
- [Strimzi Cluster Operator](https://strimzi.io/) installed with a running Kafka cluster (`kafka.strimzi.io/v1` and legacy `v1beta2` resources are supported)
- Helm 3.x

### Install with Helm

```bash
# Add the OSO DevOps Helm repository
helm repo add oso-devops https://osodevops.github.io/helm-charts/
helm repo update

# Install the operator
helm install strimzi-backup-operator oso-devops/strimzi-backup-operator \
  --namespace kafka \
  --create-namespace
```

### Create a Backup

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaBackup
metadata:
  name: my-cluster-backup
  namespace: kafka
spec:
  strimziClusterRef:
    name: my-cluster
  storage:
    type: s3
    s3:
      bucket: my-kafka-backups
      region: eu-west-1
      accessKeySecret:
        name: aws-credentials
        key: access-key-id
      secretKeySecret:
        name: aws-credentials
        key: secret-access-key
  topics:
    include:
      - "orders-*"
      - "payments-*"
    exclude:
      - "__*"
  backup:
    compression: zstd
    parallelism: 4
  logging:
    level: warn
    format: json
    modules:
      kafka_backup: warn
      rdkafka: info
  env:
    - name: RUST_LOG
      value: "kafka_backup=warn,rdkafka=info"
  schedule:
    cron: "0 2 * * *"    # Daily at 2 AM
    timezone: "Europe/London"
  retention:
    maxBackups: 30
    maxAge: "90d"
    pruneOnSchedule: true
```

### Restore from a Backup

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaRestore
metadata:
  name: my-cluster-restore
  namespace: kafka
spec:
  strimziClusterRef:
    name: my-cluster
  backupRef:
    name: my-cluster-backup
  topics:
    include:
      - "orders-*"          # glob, or "~orders-\d+" for regex
    exclude:
      - "*-internal"
  pointInTime:
    timestamp: "2026-02-12T14:30:00.000Z"
  logging:
    level: info
    format: json
  topicMapping:
    - sourceTopic: orders
      targetTopic: orders-restored
  consumerGroups:
    restore: true
```

## Custom Resource Definitions

| CRD | Short Name | API Group | Description |
|-----|-----------|-----------|-------------|
| `KafkaBackup` | `kb` | `kafkabackup.com/v1alpha1` | Defines a backup configuration with scheduling, retention, and storage |
| `KafkaRestore` | `kr` | `kafkabackup.com/v1alpha1` | Defines a restore operation with PITR, topic mapping, and consumer group restore |

### Pausing reconciliation

`KafkaBackup` and `KafkaRestore` support Strimzi's standard pause annotation.
While its value is `"true"`, the operator reports a `ReconciliationPaused`
condition but does not add a finalizer, resolve dependencies, or create/update
ConfigMaps, Jobs, or CronJobs. Removing the annotation or setting it to
`"false"` resumes normal reconciliation.

```bash
kubectl annotate kafkabackup restore-source strimzi.io/pause-reconciliation="true"
kubectl annotate kafkabackup restore-source strimzi.io/pause-reconciliation-
```

## Storage Configuration

### Amazon S3

```yaml
storage:
  type: s3
  s3:
    bucket: my-kafka-backups
    region: eu-west-1
    prefix: production/
    accessKeySecret:
      name: aws-credentials
      key: access-key-id
    secretKeySecret:
      name: aws-credentials
      key: secret-access-key
```

### Azure Blob Storage

```yaml
storage:
  type: azure
  azure:
    storageAccount: myaccount
    container: kafka-backups
    prefix: production/
    accountKeySecret:
      name: azure-credentials
      key: account-key
```

### Google Cloud Storage

```yaml
storage:
  type: gcs
  gcs:
    bucket: my-kafka-backups
    prefix: production/
    credentialsSecret:
      name: gcs-credentials
      key: service-account.json
```

### S3-Compatible (MinIO)

```yaml
storage:
  type: s3
  s3:
    bucket: kafka-backups
    endpoint: https://minio.example.com
    forcePathStyle: true
    allowHttp: false
    accessKeySecret:
      name: minio-credentials
      key: access-key-id
    secretKeySecret:
      name: minio-credentials
      key: secret-access-key
```

## Authentication

The operator automatically discovers TLS certificates and authentication credentials from your Strimzi cluster. You can also reference `KafkaUser` CRs directly:

### Automatic KafkaUser Resolution

```yaml
spec:
  strimziClusterRef:
    name: my-cluster
  authentication:
    type: tls
    kafkaUserRef:
      name: backup-user    # References a Strimzi KafkaUser CR
```

### Manual TLS Certificates

```yaml
spec:
  authentication:
    type: tls
    certificateAndKey:
      secretName: my-tls-secret
      certificate: user.crt
      key: user.key
```

### SCRAM-SHA-512

```yaml
spec:
  authentication:
    type: scram-sha-512
    username: backup-user
    passwordSecret:
      name: backup-user-password
      key: password
```

### Listener selection

Backup and restore jobs connect through the Kafka listener whose
`authentication.type` matches the resource's `spec.authentication` — SCRAM
credentials go to a `scram-sha-512` listener, client certificates to a `tls`
listener, and resources without authentication use an unauthenticated
listener. Among matching listeners, in-cluster types (`internal`,
`cluster-ip`) are preferred over external ones, and TLS-encrypted listeners
over plaintext. If no listener matches, reconciliation fails with a condition
listing the cluster's listeners.

To bypass the automatic selection, name a listener explicitly:

```yaml
spec:
  strimziClusterRef:
    name: my-cluster
    listener: external    # connect via this listener, as declared in the Kafka CR
```

## Helm Values

| Parameter | Description | Default |
|-----------|-------------|---------|
| `image.repository` | Operator container image | `ghcr.io/osodevops/strimzi-backup-operator` |
| `image.tag` | Image tag | Chart `appVersion` |
| `image.pullPolicy` | Image pull policy | `Always` |
| `replicaCount` | Number of operator replicas | `1` |
| `watchNamespaces` | Namespaces to watch (empty = all) | `[]` |
| `logging.level` | Rust log filter | `info,kafka_backup_operator=debug` |
| `logging.format` | Log output format | `json` |
| `serviceAccount.create` | Create a service account | `true` |
| `backupJobs.serviceAccountName` | Service account used by backup/restore job pods (empty = operator service account) | `""` |
| `azureWorkloadIdentity.enabled` | Enable Azure Workload Identity | `false` |
| `azureWorkloadIdentity.clientId` | Azure Managed Identity client ID | `""` |
| `metrics.enabled` | Enable Prometheus metrics | `true` |
| `metrics.serviceMonitor.enabled` | Create a Prometheus ServiceMonitor | `false` |
| `metrics.jobPodMonitor.enabled` | Create a PodMonitor for backup/restore job metrics | `false` |
| `leaderElection.enabled` | Enable leader election for HA | `false` |
| `resources.requests.cpu` | CPU request | `100m` |
| `resources.requests.memory` | Memory request | `128Mi` |
| `resources.limits.cpu` | CPU limit | `500m` |
| `resources.limits.memory` | Memory limit | `512Mi` |

### Job service accounts across namespaces

Backup and restore Jobs run in the namespace of the `KafkaBackup`/`KafkaRestore`
resource, and by default reference the service account named by
`backupJobs.serviceAccountName` (falling back to the operator's own service
account). Service accounts are namespace-scoped, so for resources created
outside the operator's namespace, set `spec.template.pod.serviceAccountName`
to a service account that exists in that namespace:

```yaml
spec:
  template:
    pod:
      serviceAccountName: kafka-backup-jobs
```

The service account only needs to exist (job pods don't call the Kubernetes
API), but it should carry any workload-identity annotations (IRSA, Azure
Workload Identity) your storage backend requires.

## Logging

The operator deployment and the backup/restore job pods are configured separately.

Configure the operator deployment log level with Helm:

```yaml
logging:
  level: "info,kafka_backup_operator=debug"
  format: json
```

Configure `kafka-backup` job logging on each `KafkaBackup` or `KafkaRestore`:

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaBackup
metadata:
  name: debug-backup
  namespace: kafka
spec:
  strimziClusterRef:
    name: my-kafka-cluster
  storage:
    type: s3
    s3:
      bucket: my-kafka-backups
      region: eu-west-1
  logging:
    level: warn
    format: json
    output: stderr
    modules:
      kafka_backup: warn
      rdkafka: info
```

For environment-based logging, use top-level `spec.env`; these entries are added
to backup and restore job containers:

```yaml
spec:
  env:
    - name: RUST_LOG
      value: "kafka_backup=warn,rdkafka=info"
```

## Monitoring

There are two independent metrics endpoints:

- The operator exposes controller health metrics on port `9090` at `/metrics`.
- Each `kafka-backup` backup/restore pod exposes operation and progress metrics
  on port `8080` by default when `spec.metrics.enabled` is not `false`.

The operator does not proxy or copy job metrics. A `ServiceMonitor` only scrapes
the operator Service; enable the chart's `PodMonitor` to discover job pods
directly across namespaces.

Operator metrics include:

| Metric | Type | Description |
|--------|------|-------------|
| `strimzi_backup_operator_build_info` | Gauge | Running operator version |
| `strimzi_backup_operator_reconciliations_total` | Counter | Reconciliations by controller and result |
| `strimzi_backup_operator_reconciliation_duration_seconds` | Histogram | Reconciliation latency by controller and result |

Job metrics include `kafka_backup_lag_records`, the low-cardinality
`kafka_backup_lag_records_sum`, snapshot progress gauges
`kafka_backup_snapshot_records_target` and
`kafka_backup_snapshot_records_remaining`, `kafka_backup_records_total`,
`kafka_backup_bytes_total`, and the runtime's storage, throughput, compression,
error, and restore metric families.

### Prometheus ServiceMonitor

```yaml
# values.yaml
metrics:
  enabled: true
  serviceMonitor:
    enabled: true
    interval: 30s
    scrapeTimeout: 10s
  jobPodMonitor:
    enabled: true
    interval: 30s
    scrapeTimeout: 10s
```

The `PodMonitor` requires the Prometheus Operator CRDs and defaults to
`namespaceSelector.any: true` because backup and restore resources may live
outside the Helm release namespace. Its endpoint path defaults to `/metrics`;
if a CR customizes `spec.metrics.path`, provide a matching custom PodMonitor.

For a one-shot backup or restore, keep the metrics endpoint alive long enough
for at least one scrape. A practical minimum is twice the PodMonitor interval:

```yaml
spec:
  metrics:
    enabled: true
    keepAliveSeconds: 60
    maxPartitionLabels: 100
```

This setting is supported by the default `kafka-backup:v0.15.12` job image.
`maxPartitionLabels` limits unique topic/partition series; set it to `0` only
when unlimited per-partition cardinality is intentional.
Durable last-success reporting should still come from the CR status or a
service-level batch metric store rather than an operator proxy.

## Disaster Recovery Workflow

```
1. SCHEDULE         2. BACKUP          3. DISASTER        4. RESTORE

+-----------+      +-----------+      +-----------+      +-----------+
|  CronJob  |----->|   Kafka   |----->|   Data    |      |   PITR    |
|  triggers |reads |   Backup  |stores|   Safe    |apply |  Restore  |
|  backup   |data  |   Job     |to S3 |   in S3   |----->|   Job     |
+-----------+      +-----------+      +-----------+      +-----------+
```

1. **Schedule** — The operator creates a CronJob based on your `KafkaBackup` schedule
2. **Backup** — The Job reads data from Kafka and writes it to object storage with compression
3. **Disaster** — Your data is safe in durable, versioned object storage
4. **Restore** — Apply a `KafkaRestore` CR with an optional point-in-time timestamp to recover

## Development

### Prerequisites

- Rust 1.88+ (stable)
- A Kubernetes cluster with Strimzi installed (for integration testing)

### Build from Source

```bash
# Build the operator
cargo build --release

# Run tests
cargo test --all-features

# Run clippy
cargo clippy --all-features -- -D warnings

# Generate CRDs
cargo run --release --bin crdgen

# Build Docker image
docker build -t strimzi-backup-operator .
```

### Local Development

```bash
# Run the operator locally against your kubeconfig
RUST_LOG=debug cargo run

# Install CRDs
kubectl apply -f deploy/crds/
```

## Contributing

Contributions are welcome. Please open an issue or submit a pull request.

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes
4. Push to the branch (`git push origin feature/my-feature`)
5. Open a pull request

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.

## Related Projects

- [Strimzi](https://strimzi.io/) — Kafka on Kubernetes
- [kafka-backup](https://github.com/osodevops/kafka-backup) — The backup engine used by this operator
- [OSO DevOps](https://osodevops.io/) — Enterprise Kafka and Kubernetes consulting
