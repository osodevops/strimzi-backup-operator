# Support, lifecycle and maintenance

The Strimzi Backup Operator and the `kafka-backup` engine are open source
(Apache-2.0 and MIT respectively). Commercial support for both — including
the operator on its own, without any enterprise-only feature in use — is
included with the **kafka-backup Enterprise licence**. The figures below are
the standard terms; a licence agreement's support schedule may extend them
(longer hours, holiday alignment, service credits) but never reduces them.

## How to get help

| Channel | Who | What to expect |
|---|---|---|
| [GitHub issues](https://github.com/osodevops/strimzi-backup-operator/issues) | everyone | Best effort. Bugs are triaged; there is no response-time commitment. |
| support@oso.sh / support portal | Enterprise licence holders | The severity targets below. Include the operator version, engine image, `kubectl get kb/kr -o yaml` and the operator log. |
| Security reports | everyone | See [SECURITY.md](SECURITY.md) — never open a public issue for a vulnerability. |

## Scope (Enterprise licence)

Covered: installation and upgrade of the operator and its Helm chart, the
`KafkaBackup`/`KafkaRestore` API, backup and restore behaviour of the engine
versions in the supported window, configuration review, and incident support
during a restore. Out of scope: operating the Kafka cluster itself, the object
store, and Strimzi (available as a separate add-on).

## Severity levels and response targets

Support hours: 08:00 to 18:00 Central European Time (CET/CEST), Monday to
Friday, excluding UK public holidays. Response targets are measured within
those hours; a ticket raised outside them is picked up at the start of the
next support period. There is no staffed 24x7 desk. Out-of-hours P1 response
is available as a per-incident call-out or as an on-call retainer for agreed
days, both priced separately in the licence agreement.

| Priority | Definition | Initial response | Workaround / mitigation |
|---|---|---|---|
| P1 | A restore needed for recovery cannot run, or backups have stopped across a production cluster | 60 minutes | 4 hours |
| P2 | Backups or restores degraded; a workaround exists | 4 hours | 1 business day |
| P3 | Non-urgent defect or how-to question | 1 business day | 3 business days |
| P4 | Enhancement request, documentation | 2 business days | 5 business days |

## Supported versions

| Component | Supported window | Notes |
|---|---|---|
| Operator | the current and the previous **minor** release (`0.N` and `0.N-1`) | Patch releases replace the previous patch. |
| Engine (`kafka-backup`) | per the [compatibility policy](README.md#compatibility): the default engine of a supported operator and any newer `0.x` engine, down to the documented minimum | Pin with `spec.image` / `backupJobs.image`. |
| Kubernetes | the last **three** minor releases supported upstream | Tested in CI on the versions listed in `scripts/e2e/`. |
| Strimzi | 0.43 and later, including 1.0 and later | The operator reads `kafka.strimzi.io/v1` resources and falls back to `v1beta2` on older releases. CI runs the end-to-end suite on Strimzi 0.46. Releases older than 0.43 work through the fallback but receive no fixes. |

Versions outside the window keep working but receive no fixes. End of support
for a minor release is announced in the CHANGELOG with the release that
succeeds it.

## Security fixes

- Reports are acknowledged within two business days and given a CVSS-based
  severity assessment within five.
- Critical and high severity issues in a supported version are fixed, or a
  mitigation is published, within ten business days; other severities with
  the next scheduled release.
- Fixes are backported to every supported minor release and published as a
  GitHub Security Advisory; Enterprise customers are notified directly.

## Maintenance and continuity

- **Release process.** Every release is cut from `main` by the process in
  [RELEASING.md](RELEASING.md): CI runs the unit, integration, engine-compatibility
  and CRD-drift checks; the release gate verifies that Cargo, the Helm chart,
  the CHANGELOG and the README agree on the version; images and binaries are
  built in CI and published with SHA-256 checksums.
- **Who can release.** At least two engineers hold release rights (repository
  admin, CI, container registry and chart repository). Either can cut a release
  or respond to an incident; the process does not depend on one person.
- **Source availability.** The operator and the engine are public, Apache-2.0
  and MIT licensed. A source-code continuity clause covering any privately
  distributed component is available in commercial agreements on request.
- **Roadmap and API stability.** Planned changes to the custom resources are
  governed by [docs/api-stability.md](docs/api-stability.md); breaking changes
  only ever ship as a new API version with a deprecation window.
- **Bus factor.** The public repositories, CI definitions, Helm chart source and
  release scripts contain everything needed to build, test and release the
  operator; no undocumented step exists.
