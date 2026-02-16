# Product Requirements Document (PRD)
# Strimzi Backup Operator

**Project:** strimzi-backup-operator  
**Author:** OSO DevOps  
**Version:** 0.1.0  
**Date:** February 2026  
**Status:** Draft  
**License:** Apache License 2.0 (aligned with Strimzi ecosystem)  
**Repository:** github.com/osodevops/strimzi-backup-operator  
**Website:** kafkabackup.com  

---

## 1. Overview

### 1.1 Problem Statement

Apache Kafka on Kubernetes, managed by Strimzi, lacks a native, first-class backup and restore solution. As highlighted in the Strimzi community (Discussion #8750 and others), the current disaster recovery story for Kafka is incomplete:

- **MirrorMaker2 is expensive and operationally complex** — it requires running a full secondary cluster, reconfiguring all clients during failover, and provides no way to revert mirroring flow after a switch.
- **PVC-based recovery is fragile** — deleting CRDs triggers garbage collection of PVCs, and recovering from node failures often requires destroying and recreating entire clusters.
- **No point-in-time recovery exists** — there is no mechanism to restore a Strimzi-managed Kafka cluster to a specific moment in time.
- **No Strimzi-native backup CRD exists** — backup/restore workflows are entirely manual or require external tooling that doesn't integrate with the Strimzi operator model.

### 1.2 Solution

The **Strimzi Backup Operator** is a Kubernetes operator, built in Rust, that extends the Strimzi ecosystem with dedicated Custom Resource Definitions (CRDs) for backup and restore operations. It follows Strimzi conventions for CRD design, API group naming, status reporting, and reconciliation patterns while leveraging the proven backup engine from OSO's kafka-backup project.

### 1.3 Goals

1. Provide a Strimzi-native backup/restore experience using CRDs under the `kafka.strimzi.io`-compatible API group.
2. Enable millisecond-precision point-in-time recovery (PITR) for Strimzi-managed Kafka clusters.
3. Integrate with existing Strimzi resources (`Kafka`, `KafkaTopic`, `KafkaUser`) for zero-configuration discovery.
4. Support multi-cloud object storage (S3, Azure Blob, GCS, MinIO).
5. Deliver high-performance backup (100+ MB/s per partition) with minimal cluster impact.
6. Follow Strimzi project conventions for CRD schema, labelling, status conditions, and operational patterns.

### 1.4 Non-Goals

- Replacing MirrorMaker2 for active-active cross-cluster replication.
- Managing Kafka cluster lifecycle (this remains the Strimzi Cluster Operator's responsibility).
- Providing a standalone CLI tool (see kafka-backup for CLI usage).
- Schema Registry backup (planned for future release).

---

## 2. Strimzi Ecosystem Integration

### 2.1 Strimzi CRD Conventions

The operator MUST follow established Strimzi patterns:

| Convention | Implementation |
|---|---|
| **API Group** | `kafkabackup.com/v1alpha1` (sub-group of strimzi.io domain) |
| **CRD Naming** | `kafkabackups.kafkabackup.com`, `kafkarestores.kafkabackup.com` |
| **Metadata Labels** | `app.kubernetes.io/name`, `app.kubernetes.io/instance`, `app.kubernetes.io/part-of: strimzi-backup`, `strimzi.io/cluster` |
| **Status Conditions** | Follow Strimzi `status.conditions[]` pattern with `type`, `status`, `reason`, `message`, `lastTransitionTime` |
| **Finalizers** | `kafkabackup.com/cleanup` for safe resource deletion |
| **Watched Namespaces** | Configurable via `STRIMZI_BACKUP_NAMESPACE` environment variable, supporting single, multi, and all-namespace modes |
| **OpenAPI v3 Schema** | Full validation on all CRD fields, following Strimzi's schema-first approach |

### 2.2 Strimzi Resource Discovery

The operator discovers Strimzi-managed Kafka clusters by watching `Kafka` custom resources:

```yaml
# Automatic discovery via Strimzi CR reference
spec:
  strimziClusterRef:
    name: my-kafka-cluster      # References a Kafka CR in the same namespace
    namespace: kafka             # Optional: cross-namespace reference
```

From the referenced `Kafka` CR, the operator automatically resolves:

- Bootstrap server addresses (internal and external listeners)
- TLS certificates from cluster CA secrets (`<cluster>-cluster-ca-cert`)
- Authentication configuration
- Number of brokers and topic metadata
- Storage class and PVC configuration

### 2.3 Strimzi User Integration

For authenticated clusters, the operator can reference a `KafkaUser` CR:

```yaml
spec:
  authentication:
    type: tls
    kafkaUserRef:
      name: backup-service-account   # References a KafkaUser CR
```

The operator reads credentials from the secret created by the Strimzi User Operator, eliminating manual credential management.

---

## 3. Custom Resource Definitions

### 3.1 KafkaBackup

The primary CRD for defining backup configurations and schedules.

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaBackup
metadata:
  name: daily-backup
  namespace: kafka
  labels:
    strimzi.io/cluster: my-kafka-cluster
spec:
  # Strimzi cluster reference (required)
  strimziClusterRef:
    name: my-kafka-cluster

  # Authentication (optional — auto-discovered from Strimzi CR if omitted)
  authentication:
    type: tls
    kafkaUserRef:
      name: backup-service-account

  # Topic selection
  topics:
    include:
      - "orders.*"
      - "payments.*"
    exclude:
      - ".*\\.internal"
      - "__.*"

  # Consumer group offset backup
  consumerGroups:
    include:
      - ".*"
    exclude: []

  # Storage destination (required)
  storage:
    type: s3
    s3:
      bucket: my-kafka-backups
      region: eu-west-1
      prefix: "strimzi/my-kafka-cluster/"
      credentialsSecret:
        name: backup-s3-credentials
        key: aws-credentials
    # Alternative: Azure Blob
    # type: azure
    # azure:
    #   container: kafka-backups
    #   storageAccount: mystorageaccount
    #   credentialsSecret:
    #     name: backup-azure-credentials
    # Alternative: GCS
    # type: gcs
    # gcs:
    #   bucket: my-kafka-backups
    #   credentialsSecret:
    #     name: backup-gcs-credentials

  # Backup configuration
  backup:
    compression: zstd           # none | gzip | snappy | lz4 | zstd
    encryption:
      enabled: false
      keySecret:
        name: backup-encryption-key
        key: aes-256-key
    segmentSize: 268435456      # 256MB — max segment file size before rotation
    parallelism: 4              # Number of concurrent partition backup threads

  # Schedule (cron format, optional — if omitted, runs once)
  schedule:
    cron: "0 2 * * *"           # Daily at 2:00 AM
    timezone: "UTC"
    suspend: false

  # Retention policy
  retention:
    maxBackups: 30              # Keep last 30 backups
    maxAge: "30d"               # Or keep backups for 30 days
    pruneOnSchedule: true       # Auto-prune expired backups after each scheduled run

  # Resource requirements for the backup pod
  resources:
    requests:
      cpu: "500m"
      memory: "1Gi"
    limits:
      cpu: "2"
      memory: "4Gi"

  # Template for backup pod customisation (follows Strimzi template pattern)
  template:
    pod:
      metadata:
        labels:
          custom-label: backup-pod
        annotations:
          prometheus.io/scrape: "true"
          prometheus.io/port: "9090"
      affinity: {}
      tolerations: []
      securityContext:
        runAsNonRoot: true
        runAsUser: 1001
    container:
      env:
        - name: RUST_LOG
          value: "info"

status:
  conditions:
    - type: Ready
      status: "True"
      reason: BackupScheduled
      message: "Next backup scheduled for 2026-02-14T02:00:00Z"
      lastTransitionTime: "2026-02-13T08:30:00Z"
  lastBackup:
    id: "backup-20260213-020000"
    startTime: "2026-02-13T02:00:00Z"
    completionTime: "2026-02-13T02:15:23Z"
    status: Completed
    sizeBytes: 53687091200
    topicsBackedUp: 12
    partitionsBackedUp: 96
    oldestTimestamp: "2026-01-14T02:00:00Z"
    newestTimestamp: "2026-02-13T01:59:59.999Z"
  backupHistory:
    - id: "backup-20260213-020000"
      status: Completed
      startTime: "2026-02-13T02:00:00Z"
      sizeBytes: 53687091200
    - id: "backup-20260212-020000"
      status: Completed
      startTime: "2026-02-12T02:00:00Z"
      sizeBytes: 52428800000
  observedGeneration: 3
  nextScheduledBackup: "2026-02-14T02:00:00Z"
```

### 3.2 KafkaRestore

The CRD for restoring data from a backup, including point-in-time recovery.

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaRestore
metadata:
  name: restore-to-dr
  namespace: kafka
  labels:
    strimzi.io/cluster: dr-kafka-cluster
spec:
  # Target Strimzi cluster (required)
  strimziClusterRef:
    name: dr-kafka-cluster

  # Authentication for target cluster
  authentication:
    type: tls
    kafkaUserRef:
      name: restore-service-account

  # Backup source reference
  backupRef:
    name: daily-backup            # References a KafkaBackup CR
    backupId: "backup-20260213-020000"  # Specific backup snapshot (optional — latest if omitted)

  # Point-in-time recovery (optional)
  pointInTime:
    timestamp: "2026-02-13T01:30:00.000Z"   # Restore up to this exact millisecond
    # Alternative: use a duration offset
    # offsetFromEnd: "2h"                    # Restore to 2 hours before the backup end

  # Topic mapping (optional — restore to same topic names by default)
  topicMapping:
    # Rename topics during restore
    - sourceTopic: "orders"
      targetTopic: "orders-restored"
    - sourceTopic: "payments"
      targetTopic: "payments-restored"

  # Consumer group offset restore
  consumerGroups:
    restore: true                # Restore consumer group offsets
    mapping:
      - sourceGroup: "order-processor"
        targetGroup: "order-processor"   # Same name, offsets adjusted for PITR

  # Restore behaviour
  restore:
    topicCreation: auto          # auto | manual — auto creates topics if missing
    existingTopicPolicy: fail    # fail | append | overwrite
    parallelism: 4

  # Resource requirements for the restore pod
  resources:
    requests:
      cpu: "1"
      memory: "2Gi"
    limits:
      cpu: "4"
      memory: "8Gi"

  # Template (same pattern as KafkaBackup)
  template:
    pod:
      metadata:
        labels:
          custom-label: restore-pod

status:
  conditions:
    - type: Ready
      status: "True"
      reason: RestoreCompleted
      message: "Restore completed successfully"
      lastTransitionTime: "2026-02-13T09:45:00Z"
  restore:
    startTime: "2026-02-13T09:30:00Z"
    completionTime: "2026-02-13T09:45:00Z"
    status: Completed
    restoredTopics: 8
    restoredPartitions: 64
    restoredBytes: 42949672960
    pointInTimeTarget: "2026-02-13T01:30:00.000Z"
    actualPointInTime: "2026-02-13T01:29:59.997Z"
  observedGeneration: 1
```

### 3.3 KafkaBackupSchedule (Future — v0.2.0)

A higher-level CRD for managing complex backup policies across multiple clusters.

```yaml
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaBackupSchedule
metadata:
  name: enterprise-backup-policy
spec:
  clusterSelector:
    matchLabels:
      environment: production
  backupTemplate:
    spec:
      # ... KafkaBackup spec template applied to all matched clusters
  schedule:
    cron: "0 */6 * * *"
```

---

## 4. Architecture

### 4.1 High-Level Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    Kubernetes Cluster                         │
│                                                              │
│  ┌─────────────────────┐    ┌─────────────────────────────┐  │
│  │  Strimzi Cluster    │    │  Strimzi Backup Operator    │  │
│  │  Operator           │    │  (Rust)                     │  │
│  │                     │    │                             │  │
│  │  Manages:           │    │  Watches:                   │  │
│  │  - Kafka CR         │◄───│  - Kafka CRs (read-only)   │  │
│  │  - KafkaTopic CR    │    │  - KafkaBackup CRs          │  │
│  │  - KafkaUser CR     │    │  - KafkaRestore CRs         │  │
│  │  - KafkaMM2 CR      │    │                             │  │
│  └─────────────────────┘    │  Creates:                   │  │
│                              │  - Backup Jobs/Pods         │  │
│  ┌─────────────────────┐    │  - Restore Jobs/Pods        │  │
│  │  Kafka Cluster      │    │  - Metrics endpoints        │  │
│  │  (Strimzi-managed)  │    └──────────┬──────────────────┘  │
│  │                     │               │                     │
│  │  ┌──────┐ ┌──────┐ │    ┌──────────▼──────────┐          │
│  │  │Broker│ │Broker│ │    │  Backup/Restore Pod  │          │
│  │  │  0   │ │  1   │◄├────│  (Rust binary)       │          │
│  │  └──────┘ └──────┘ │    │                      │          │
│  └─────────────────────┘    └──────────┬───────────┘         │
│                                        │                     │
└────────────────────────────────────────┼─────────────────────┘
                                         │
                              ┌──────────▼───────────┐
                              │  Object Storage      │
                              │  - AWS S3             │
                              │  - Azure Blob         │
                              │  - Google GCS         │
                              │  - MinIO / S3-compat  │
                              └──────────────────────┘
```

### 4.2 Operator Components

| Component | Language | Description |
|---|---|---|
| **Backup Operator Controller** | Rust (kube-rs) | Watches CRDs, reconciles state, manages Jobs |
| **Backup Engine** | Rust | High-performance Kafka consumer that writes segments to object storage. Reuses core from kafka-backup. |
| **Restore Engine** | Rust | Reads segments from object storage, produces to target cluster with offset mapping. |
| **Metrics Exporter** | Rust | Prometheus-compatible metrics endpoint for backup/restore operations. |

### 4.3 Reconciliation Loop

The operator follows the standard Kubernetes controller pattern:

1. **Watch** — monitor `KafkaBackup` and `KafkaRestore` CRs for changes.
2. **Resolve** — read the referenced `Kafka` CR to discover cluster configuration (bootstrap servers, TLS certs, auth).
3. **Plan** — determine required actions (create Job, update status, prune old backups).
4. **Execute** — create or update Kubernetes Jobs that run the backup/restore engine binary.
5. **Report** — update CR `.status` with conditions, progress, and metrics.

Reconciliation is triggered by:
- CR creation, modification, or deletion
- CronJob schedule triggers
- Referenced `Kafka` CR changes (e.g., broker count change)
- Backup Job completion or failure

### 4.4 Data Flow — Backup

1. Operator reads `KafkaBackup` CR and resolves the Strimzi `Kafka` CR.
2. Operator creates a Kubernetes `Job` with the backup engine container.
3. Backup engine connects to Kafka using discovered bootstrap servers and TLS certs.
4. Engine assigns itself all partitions for selected topics (no consumer group — direct assignment).
5. Records are consumed, batched, compressed (zstd), and written as segment files to object storage.
6. A manifest file is written with metadata: topic, partition, offset ranges, timestamps, checksums.
7. Job completes; operator updates `KafkaBackup` status.

### 4.5 Data Flow — Restore

1. Operator reads `KafkaRestore` CR and resolves target `Kafka` CR and source `KafkaBackup` CR.
2. Operator creates a Kubernetes `Job` with the restore engine container.
3. Restore engine reads the backup manifest from object storage.
4. If PITR is specified, engine filters segments and records to the target timestamp.
5. Records are produced to the target cluster, respecting topic mapping and partition assignment.
6. Consumer group offsets are committed if configured.
7. Job completes; operator updates `KafkaRestore` status.

---

## 5. Strimzi Compatibility Matrix

| Strimzi Version | Kafka Version | Operator Support | Notes |
|---|---|---|---|
| 0.43.x | 3.8.x | Full | Current target |
| 0.42.x | 3.7.x | Full | |
| 0.41.x | 3.7.x | Full | |
| 0.40.x | 3.6.x | Best-effort | Minimum supported |
| < 0.40.x | < 3.6.x | Unsupported | |

The operator detects the Strimzi version from the `Kafka` CR's `apiVersion` and adjusts behaviour accordingly (e.g., KRaft vs ZooKeeper mode detection).

---

## 6. Security

### 6.1 RBAC Requirements

The operator requires the following Kubernetes RBAC permissions:

```yaml
# ClusterRole for the operator
rules:
  # Read Strimzi resources
  - apiGroups: ["kafka.strimzi.io"]
    resources: ["kafkas", "kafkatopics", "kafkausers"]
    verbs: ["get", "list", "watch"]
  # Manage backup CRDs
  - apiGroups: ["kafkabackup.com"]
    resources: ["kafkabackups", "kafkarestores", "kafkabackups/status", "kafkarestores/status"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Create backup/restore jobs
  - apiGroups: ["batch"]
    resources: ["jobs", "cronjobs"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Read secrets for credentials
  - apiGroups: [""]
    resources: ["secrets"]
    verbs: ["get", "list", "watch"]
  # Read configmaps, manage events
  - apiGroups: [""]
    resources: ["configmaps", "events", "pods"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]
```

### 6.2 Encryption

- **In-transit**: TLS for all Kafka connections, leveraging Strimzi cluster CA certificates.
- **At-rest**: Optional AES-256-GCM encryption of backup segments before storage upload.
- **Key management**: Encryption keys stored in Kubernetes Secrets, with future support for external KMS (AWS KMS, Azure Key Vault, HashiCorp Vault).

### 6.3 Network Policies

The operator supports Strimzi's network policy integration. Backup/restore pods are labelled to work with existing Strimzi `NetworkPolicy` resources that restrict Kafka broker access.

---

## 7. Observability

### 7.1 Prometheus Metrics

Exposed on `:9090/metrics` following Prometheus conventions:

| Metric | Type | Description |
|---|---|---|
| `strimzi_backup_records_total` | Counter | Total records backed up |
| `strimzi_backup_bytes_total` | Counter | Total bytes backed up |
| `strimzi_backup_duration_seconds` | Histogram | Backup duration |
| `strimzi_backup_last_success_timestamp` | Gauge | Timestamp of last successful backup |
| `strimzi_backup_last_failure_timestamp` | Gauge | Timestamp of last failed backup |
| `strimzi_restore_records_total` | Counter | Total records restored |
| `strimzi_restore_bytes_total` | Counter | Total bytes restored |
| `strimzi_restore_duration_seconds` | Histogram | Restore duration |
| `strimzi_backup_storage_bytes` | Gauge | Total storage used by backups |
| `strimzi_backup_lag_seconds` | Gauge | Time since last successful backup |

### 7.2 Grafana Dashboard

A pre-built Grafana dashboard JSON is provided in the repository under `deploy/grafana/`, compatible with the Strimzi monitoring stack.

### 7.3 Kubernetes Events

The operator emits Kubernetes Events on the `KafkaBackup`/`KafkaRestore` CRs for:
- Backup started, completed, failed
- Restore started, completed, failed
- Retention pruning executed
- Configuration errors detected
- Strimzi cluster connectivity issues

---

## 8. Installation

### 8.1 Helm Chart

```bash
helm repo add strimzi-backup https://charts.kafkabackup.com
helm install strimzi-backup-operator strimzi-backup/strimzi-backup-operator \
  --namespace kafka \
  --set watchNamespaces="{kafka,kafka-staging}"
```

### 8.2 YAML Manifests

```bash
kubectl apply -f https://github.com/osodevops/strimzi-backup-operator/releases/latest/download/install.yaml
```

### 8.3 OLM (Operator Lifecycle Manager)

Published to OperatorHub.io for OpenShift and OLM-enabled clusters:

```bash
kubectl apply -f https://operatorhub.io/install/strimzi-backup-operator.yaml
```

---

## 9. Milestones

### v0.1.0-alpha — Foundation (8 weeks)

- [ ] Rust operator scaffolding with kube-rs
- [ ] `KafkaBackup` CRD with OpenAPI v3 schema validation
- [ ] `KafkaRestore` CRD with OpenAPI v3 schema validation
- [ ] Strimzi `Kafka` CR discovery and TLS certificate resolution
- [ ] Basic backup engine: consume → compress → write to S3
- [ ] Basic restore engine: read from S3 → decompress → produce
- [ ] Status condition reporting following Strimzi conventions
- [ ] Unit and integration test suite

### v0.1.0 — MVP (4 weeks after alpha)

- [ ] Scheduled backups via CronJob generation
- [ ] Point-in-time recovery (PITR) with millisecond precision
- [ ] Multi-cloud storage (S3, Azure Blob, GCS)
- [ ] Retention policy enforcement
- [ ] Prometheus metrics endpoint
- [ ] Helm chart and YAML installation manifests
- [ ] Documentation site integration with kafkabackup.com
- [ ] End-to-end test suite with Strimzi test containers

### v0.2.0 — Production Ready (6 weeks after v0.1.0)

- [ ] `KafkaBackupSchedule` CRD for multi-cluster policies
- [ ] Consumer group offset backup and restore
- [ ] Topic mapping during restore (rename, repartition)
- [ ] At-rest encryption (AES-256-GCM)
- [ ] `KafkaUser` CR integration for automatic credential discovery
- [ ] Grafana dashboard
- [ ] OLM / OperatorHub publication
- [ ] Performance benchmarks and tuning guide

### v0.3.0 — Enterprise (8 weeks after v0.2.0)

- [ ] Schema Registry backup and restore
- [ ] External KMS integration (AWS KMS, Azure Key Vault, Vault)
- [ ] RBAC and multi-tenancy support
- [ ] Audit logging
- [ ] Data masking during restore
- [ ] Web UI dashboard (optional component)
- [ ] Commercial license and enterprise support tiers

---

## 10. SEO & Branding Strategy

### 10.1 Naming

- **Project name**: `strimzi-backup-operator`
- **CRD prefix**: `KafkaBackup`, `KafkaRestore` (consistent with Strimzi's `Kafka*` naming)
- **Helm chart**: `strimzi-backup/strimzi-backup-operator`
- **Docker image**: `ghcr.io/osodevops/strimzi-backup-operator`
- **Documentation URL**: `kafkabackup.com/strimzi` or `strimzi.kafkabackup.com`

### 10.2 SEO Target Keywords

| Primary Keywords | Secondary Keywords |
|---|---|
| strimzi backup | strimzi disaster recovery |
| strimzi kafka backup | kafka kubernetes backup |
| strimzi backup operator | strimzi point in time recovery |
| strimzi restore | kafka backup operator kubernetes |
| kafka backup kubernetes | strimzi DR |

### 10.3 Community Presence

- Publish announcement on Strimzi GitHub Discussions
- Submit talk proposals to KubeCon, Kafka Summit, Strimzi community calls
- Create blog posts on kafkabackup.com targeting SEO keywords
- Publish to OperatorHub.io and Artifact Hub
- Engage in Strimzi Slack/CNCF channels

### 10.4 Differentiation from kafka-backup

| Aspect | kafka-backup | strimzi-backup-operator |
|---|---|---|
| **Target audience** | Any Kafka deployment | Strimzi/Kubernetes-native deployments |
| **Interface** | CLI + generic K8s operator | Strimzi-native CRDs |
| **Discovery** | Manual configuration | Auto-discovers from Strimzi CRs |
| **Authentication** | Manual credential config | Integrates with KafkaUser CRs |
| **Installation** | Standalone binary/container | Helm, OLM, Strimzi ecosystem |
| **Branding** | kafkabackup.com | strimzi-backup-operator + kafkabackup.com |

---

## 11. Technical Constraints

- **Rust toolchain**: Stable Rust (latest MSRV: 1.75+), async runtime: Tokio.
- **Kubernetes client**: `kube-rs` with derive macros for CRD generation.
- **Minimum Kubernetes**: 1.27+ (for CRD validation features).
- **Minimum Strimzi**: 0.40.x.
- **Container base image**: `distroless/cc-debian12` for minimal attack surface.
- **CI/CD**: GitHub Actions with cross-compilation for `linux/amd64` and `linux/arm64`.

---

## 12. Open Questions

1. **API Group**: Should we use `kafkabackup.com` (sub-domain of strimzi.io) or `kafkabackup.io` (independent)? Using the strimzi.io domain requires community approval or clear branding distinction.
2. **Strimzi contribution path**: Should this be proposed as a Strimzi sub-project, or remain an independent OSO project that integrates with Strimzi?
3. **KRaft-only support**: Should v0.1.0 support both ZooKeeper and KRaft mode Strimzi clusters, or target KRaft-only given ZooKeeper deprecation?
4. **Incremental backups**: Should v0.1.0 support incremental backups (only new offsets since last backup), or full-snapshot only?
5. **Strimzi test framework**: Should we use the Strimzi system test framework (Java-based) for compatibility testing, or build a Rust-native test harness?

---

## Appendix A: Related Work

| Project | Approach | Limitations |
|---|---|---|
| MirrorMaker2 | Active replication between clusters | Expensive (full cluster required), no PITR, complex failover |
| Confluent Replicator | Commercial cross-cluster replication | Vendor lock-in, no PITR, licensed |
| Lenses.io Backup | Topic backup via Lenses platform | Platform dependency, not Kubernetes-native |
| kafka-backup (OSO) | CLI + generic K8s operator | Not Strimzi-native, manual configuration |
| Volume snapshots (PVC) | Kubernetes CSI volume snapshots | Broker-level only, no topic granularity, requires downtime |

---

## Appendix B: Example End-to-End Workflow

```bash
# 1. Install Strimzi (if not already installed)
kubectl apply -f https://strimzi.io/install/latest

# 2. Deploy a Kafka cluster
kubectl apply -f - <<EOF
apiVersion: kafka.strimzi.io/v1beta2
kind: Kafka
metadata:
  name: production-cluster
  namespace: kafka
spec:
  kafka:
    replicas: 3
    storage:
      type: persistent-claim
      size: 100Gi
  zookeeper:
    replicas: 3
    storage:
      type: persistent-claim
      size: 10Gi
  entityOperator:
    topicOperator: {}
    userOperator: {}
EOF

# 3. Install the Strimzi Backup Operator
helm install strimzi-backup-operator strimzi-backup/strimzi-backup-operator -n kafka

# 4. Create a backup configuration
kubectl apply -f - <<EOF
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaBackup
metadata:
  name: daily-backup
  namespace: kafka
spec:
  strimziClusterRef:
    name: production-cluster
  topics:
    include: [".*"]
    exclude: ["__.*"]
  storage:
    type: s3
    s3:
      bucket: my-kafka-backups
      region: eu-west-1
      credentialsSecret:
        name: aws-creds
  schedule:
    cron: "0 2 * * *"
  retention:
    maxAge: "30d"
EOF

# 5. Trigger a manual backup
kubectl annotate kafkabackup daily-backup kafkabackup.com/trigger=now -n kafka

# 6. Restore to a point in time
kubectl apply -f - <<EOF
apiVersion: kafkabackup.com/v1alpha1
kind: KafkaRestore
metadata:
  name: pitr-restore
  namespace: kafka
spec:
  strimziClusterRef:
    name: dr-cluster
  backupRef:
    name: daily-backup
  pointInTime:
    timestamp: "2026-02-13T01:30:00.000Z"
  restore:
    topicCreation: auto
    existingTopicPolicy: fail
EOF

# 7. Monitor progress
kubectl get kafkarestore pitr-restore -n kafka -o jsonpath='{.status.conditions}'
```

---

*This PRD is a living document. Updates will be tracked in the project repository.*

*© 2026 OSO DevOps — kafkabackup.com*
