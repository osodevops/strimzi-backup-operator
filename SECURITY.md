# Security policy

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a security problem.

Report it to security@oso.sh (or through
[GitHub private vulnerability reporting](https://github.com/osodevops/strimzi-backup-operator/security/advisories/new)
on this repository). Include the operator and engine versions, a description of
the issue, and steps to reproduce if you have them.

You will receive an acknowledgement within two business days and a
severity assessment within five business days. We follow coordinated
disclosure: we ask for up to 90 days to ship a fix before details are
published, and we credit reporters in the advisory unless they prefer not to
be named.

## Supported versions

Security fixes are provided for the versions listed under "Supported versions"
in [SUPPORT.md](SUPPORT.md) — the current and previous minor release of the
operator — and are backported to each of them.

## How fixes are published

- A GitHub Security Advisory on this repository (and on
  [osodevops/kafka-backup](https://github.com/osodevops/kafka-backup) when the
  engine is affected), with a CVE where applicable.
- A patch release for every supported minor version, noted in the CHANGELOG.
- Direct notification to Enterprise licence holders.

## Scope notes

- Backup and restore Job pods run with the ServiceAccount and credentials you
  give them; the operator never reads Kafka data itself. Storage credentials
  are passed to Jobs as Kubernetes Secrets or via workload identity (IRSA,
  Azure Workload Identity, GCP Workload Identity) — the latter is preferred.
- Release artefacts (images, binaries, Helm chart) are built in CI and
  published with SHA-256 checksums; verify them before deploying.
