# Strimzi Backup Operator

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.27%2B-326ce5.svg)](https://kubernetes.io)

A Strimzi-native Kubernetes operator for **Strimzi backup** and disaster recovery of Apache Kafka clusters. Provides dedicated CRDs for automated Kafka backup scheduling, point-in-time recovery, and multi-cloud storage — fully integrated with the Strimzi ecosystem.

## Why Strimzi Backup?

Strimzi makes running Apache Kafka on Kubernetes straightforward, but **backup and disaster recovery remain unsolved problems** in the Strimzi ecosystem:

- **MirrorMaker2 is not a backup** — it requires a full secondary cluster, expensive cross-cluster replication, and complex client failover procedures.
- **PVC snapshots are fragile** — deleting Strimzi CRDs triggers garbage collection of PVCs, and node failures can result in permanent data loss.
- **No point-in-time recovery** — there is no native mechanism to restore a Strimzi Kafka cluster to a specific moment in time.
- **No Strimzi-native backup CRD** — backup workflows are entirely manual or require external tools that don't integrate with the Strimzi operator model.

The **Strimzi Backup Operator** solves these problems with a purpose-built Kubernetes operator that follows Strimzi conventions and extends the Strimzi ecosystem with first-class backup and restore capabilities.

## Features

- **Strimzi-native CRDs** — `KafkaBackup` and `KafkaRestore` custom resources under the `backup.strimzi.io` API group, following Strimzi conventions for status conditions, labels, and finalizers
- **Auto-discovery of Strimzi resources** — automatically resolves bootstrap servers, TLS certificates, and KafkaUser credentials from your existing Strimzi `Kafka` CRs
- **Scheduled Strimzi backups** — cron-based scheduling with timezone support via Kubernetes CronJobs
- **Point-in-time recovery (PITR)** — restore your Strimzi Kafka cluster to any millisecond-precision timestamp
- **Multi-cloud storage** — back up to Amazon S3, Azure Blob Storage, Google Cloud Storage, or any S3-compatible store (MinIO, Ceph RGW)
- **Topic filtering** — include/exclude topics using regex patterns
- **Topic mapping** — rename topics during restore for migration or testing scenarios
- **Consumer group offset restore** — restore consumer group offsets with optional group remapping
- **Retention policies** — automatic pruning of old backups by count or age
- **Compression** — gzip, snappy, lz4, or zstd compression for storage efficiency
- **Prometheus metrics** — built-in observability with backup/restore counters, duration histograms, and storage gauges
- **Pod template customisation** — full control over backup/restore Job pods (affinity, tolerations, security context, environment variables)
- **Azure Workload Identity** — native support for passwordless Azure authentication

## Architecture

The Strimzi Backup Operator creates Kubernetes Jobs (or CronJobs for scheduled backups) that run the `kafka-backup` CLI to perform backup and restore operations. This provides resource isolation, failure isolation, and pod-level customisation.

```
┌──────────────────────────────────────────────────────┐
│                  Kubernetes Cluster                   │
│                                                      │
│  ┌──────────────┐       ┌────────────────────────┐   │
│  │   Strimzi     │       │  Strimzi Backup        │   │
│  │   Cluster     │◄──────│  Operator              │   │
│  │   Operator    │ reads │  (watches KafkaBackup   │   │
│  │              │ Kafka  │   & KafkaRestore CRs)   │   │
│  └──────┬───────┘  CRs  └──────────┬─────────────┘   │
│         │                          │                  │
│         ▼                          ▼ creates          │
│  ┌──────────────┐       ┌────────────────────────┐   │
│  │   Kafka CR    │       │  Backup/Restore Jobs   │   │
│  │   + Brokers   │◄──────│  (kafka-backup CLI)    │   │
│  │   + ZooKeeper │ reads │                        │   │
│  └──────────────┘  data  └──────────┬─────────────┘   │
│                                     │                  │
│                                     ▼ writes/reads     │
│                          ┌────────────────────────┐   │
│                          │  Object Storage         │   │
│                          │  (S3 / Azure / GCS)     │   │
│                          └────────────────────────┘   │
└──────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- Kubernetes 1.27+
- [Strimzi Cluster Operator](https://strimzi.io/) installed with a running Kafka cluster
- Helm 3.x

### Install with Helm

```bash
helm install strimzi-backup-operator deploy/helm/strimzi-backup-operator \
  --namespace kafka \
  --create-namespace
```

### Create a Strimzi Backup

```yaml
apiVersion: backup.strimzi.io/v1alpha1
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
      credentialsSecret:
        name: aws-credentials
        key: credentials
  topics:
    include:
      - "orders.*"
      - "payments.*"
    exclude:
      - "__.*"
  backup:
    compression: zstd
    parallelism: 4
  schedule:
    cron: "0 2 * * *"    # Daily at 2 AM
    timezone: "Europe/London"
  retention:
    maxBackups: 30
    maxAge: "90d"
    pruneOnSchedule: true
```

### Restore a Strimzi Backup

```yaml
apiVersion: backup.strimzi.io/v1alpha1
kind: KafkaRestore
metadata:
  name: my-cluster-restore
  namespace: kafka
spec:
  strimziClusterRef:
    name: my-cluster
  backupRef:
    name: my-cluster-backup
  pointInTime:
    timestamp: "2026-02-12T14:30:00.000Z"
  topicMapping:
    - sourceTopic: orders
      targetTopic: orders-restored
  consumerGroups:
    restore: true
```

## Custom Resource Definitions

| CRD | Short Name | API Group | Description |
|-----|-----------|-----------|-------------|
| `KafkaBackup` | `kb` | `backup.strimzi.io/v1alpha1` | Defines a Strimzi backup configuration with scheduling, retention, and storage |
| `KafkaRestore` | `kr` | `backup.strimzi.io/v1alpha1` | Defines a restore operation with PITR, topic mapping, and consumer group restore |

## Storage Configuration

### Amazon S3

```yaml
storage:
  type: s3
  s3:
    bucket: my-kafka-backups
    region: eu-west-1
    prefix: production/
    credentialsSecret:
      name: aws-credentials
      key: credentials
```

### Azure Blob Storage

```yaml
storage:
  type: azure
  azure:
    storageAccount: myaccount
    container: kafka-backups
    prefix: production/
    credentialsSecret:
      name: azure-credentials
      key: connection-string
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
    credentialsSecret:
      name: minio-credentials
      key: credentials
```

## Strimzi Backup Authentication

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

## Helm Values

| Parameter | Description | Default |
|-----------|-------------|---------|
| `image.repository` | Operator container image | `ghcr.io/osodevops/strimzi-backup-operator` |
| `image.tag` | Image tag | Chart `appVersion` |
| `image.pullPolicy` | Image pull policy | `Always` |
| `replicaCount` | Number of operator replicas | `1` |
| `watchNamespaces` | Namespaces to watch (empty = all) | `[]` |
| `logging.level` | Rust log filter | `info,strimzi_backup_operator=debug` |
| `logging.format` | Log output format | `json` |
| `serviceAccount.create` | Create a service account | `true` |
| `azureWorkloadIdentity.enabled` | Enable Azure Workload Identity | `false` |
| `azureWorkloadIdentity.clientId` | Azure Managed Identity client ID | `""` |
| `metrics.enabled` | Enable Prometheus metrics | `true` |
| `metrics.serviceMonitor.enabled` | Create a Prometheus ServiceMonitor | `false` |
| `leaderElection.enabled` | Enable leader election for HA | `false` |
| `resources.requests.cpu` | CPU request | `100m` |
| `resources.requests.memory` | Memory request | `128Mi` |
| `resources.limits.cpu` | CPU limit | `500m` |
| `resources.limits.memory` | Memory limit | `512Mi` |

## Monitoring

The operator exposes Prometheus metrics on port `9090` at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `strimzi_backup_records_total` | Counter | Total records backed up |
| `strimzi_backup_bytes_total` | Counter | Total bytes backed up |
| `strimzi_backup_duration_seconds` | Histogram | Backup duration |
| `strimzi_backup_last_success_timestamp` | Gauge | Last successful backup time |
| `strimzi_backup_last_failure_timestamp` | Gauge | Last failed backup time |
| `strimzi_backup_storage_bytes` | Gauge | Total storage used |
| `strimzi_backup_lag_seconds` | Gauge | Time since last backup |
| `strimzi_restore_records_total` | Counter | Total records restored |
| `strimzi_restore_bytes_total` | Counter | Total bytes restored |
| `strimzi_restore_duration_seconds` | Histogram | Restore duration |

### Prometheus ServiceMonitor

```yaml
# values.yaml
metrics:
  enabled: true
  serviceMonitor:
    enabled: true
    interval: 30s
    scrapeTimeout: 10s
```

## Disaster Recovery Workflow

```
1. SCHEDULE          2. BACKUP             3. DISASTER           4. RESTORE
   ┌─────────┐         ┌─────────┐          ┌─────────┐          ┌─────────┐
   │ CronJob  │────────►│  Kafka   │─────────►│  Data   │          │  PITR   │
   │ triggers │  reads  │  Backup  │  stores  │  Safe   │  apply   │ Restore │
   │ backup   │  data   │  Job     │  to S3   │  in S3  │────────►│  Job    │
   └─────────┘         └─────────┘          └─────────┘          └─────────┘
```

1. **Schedule** — The operator creates a CronJob based on your `KafkaBackup` schedule
2. **Backup** — The Job reads data from Kafka and writes it to object storage with compression
3. **Disaster** — Your data is safe in durable, versioned object storage
4. **Restore** — Apply a `KafkaRestore` CR with an optional point-in-time timestamp to recover

## Development

### Prerequisites

- Rust 1.75+ (stable)
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
