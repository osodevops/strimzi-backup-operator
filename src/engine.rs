//! The kafka-backup engine image that backup/restore Jobs run, and the
//! operator's compatibility policy for it.
//!
//! The image a Job runs is resolved in this order: `spec.image` on the
//! `KafkaBackup`/`KafkaRestore`, then the operator-wide default from
//! `BACKUP_JOB_IMAGE` (Helm `backupJobs.image`), then the release's
//! compiled-in default. See the README section "Compatibility".

use std::fmt;

use crate::error::{Error, Result};

/// Default engine image for this operator release. Pinned to a public,
/// current kafka-backup release so backup/restore job behaviour is
/// deterministic and the image is anonymously pullable by Kubernetes.
/// Change it with `scripts/bump-engine.sh`, never by hand.
pub const DEFAULT_BACKUP_IMAGE: &str = "osodevops/kafka-backup:v0.19.1";

/// Oldest engine the generated config is known to degrade gracefully on:
/// from kafka-backup 0.16.0 an unknown config key is warned about instead of
/// failing the run. A Job is still created for an older engine, but the
/// resource gets `EngineVersionSupported=False`.
pub const MIN_SUPPORTED_ENGINE: EngineVersion = EngineVersion {
    major: 0,
    minor: 16,
    patch: 0,
};

/// Environment variable the Helm chart renders from `backupJobs.image`.
pub const JOB_IMAGE_ENV: &str = "BACKUP_JOB_IMAGE";
/// Environment variable the Helm chart renders from `backupJobs.imagePullPolicy`.
pub const JOB_IMAGE_PULL_POLICY_ENV: &str = "BACKUP_JOB_IMAGE_PULL_POLICY";

const PULL_POLICIES: [&str; 3] = ["Always", "IfNotPresent", "Never"];

/// A `major.minor.patch` engine release, as carried in the image tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EngineVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl EngineVersion {
    /// Parse `x.y.z` or `vx.y.z`. Anything else — pre-release suffixes,
    /// `latest`, a digest — is `None`: the policy only speaks about releases.
    pub fn parse(tag: &str) -> Option<Self> {
        let tag = tag.strip_prefix('v').unwrap_or(tag);
        let mut parts = tag.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for EngineVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Where the operator-wide default image came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSource {
    CompiledIn,
    Env,
}

impl ImageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageSource::CompiledIn => "compiled-in",
            ImageSource::Env => "env",
        }
    }
}

/// Operator-wide engine image settings, read once at start-up from the
/// environment the Helm chart renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineImageConfig {
    /// Image used when a resource does not set `spec.image`.
    pub default_image: String,
    pub source: ImageSource,
    /// `imagePullPolicy` for Job pods; `None` leaves the Kubernetes default
    /// (`IfNotPresent` for a pinned tag, `Always` for `:latest`).
    pub pull_policy: Option<String>,
}

impl Default for EngineImageConfig {
    fn default() -> Self {
        Self::compiled_in()
    }
}

impl EngineImageConfig {
    /// The release's compiled-in default, as used outside Helm.
    pub fn compiled_in() -> Self {
        Self {
            default_image: DEFAULT_BACKUP_IMAGE.to_string(),
            source: ImageSource::CompiledIn,
            pull_policy: None,
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Like [`from_env`](Self::from_env) but with an injectable variable
    /// lookup, so tests do not have to mutate the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };

        let mut config = Self::compiled_in();

        if let Some(image) = get(JOB_IMAGE_ENV) {
            if image.chars().any(char::is_whitespace) {
                return Err(Error::InvalidConfig(format!(
                    "{JOB_IMAGE_ENV}: image reference must not contain whitespace: {image:?}"
                )));
            }
            config.default_image = image;
            config.source = ImageSource::Env;
        }

        if let Some(policy) = get(JOB_IMAGE_PULL_POLICY_ENV) {
            if !PULL_POLICIES.contains(&policy.as_str()) {
                return Err(Error::InvalidConfig(format!(
                    "{JOB_IMAGE_PULL_POLICY_ENV}: expected one of {}, got {policy:?}",
                    PULL_POLICIES.join(", ")
                )));
            }
            config.pull_policy = Some(policy);
        }

        Ok(config)
    }

    /// The image a Job runs: `spec.image` wins over the operator-wide default.
    pub fn resolve<'a>(&'a self, spec_image: Option<&'a str>) -> &'a str {
        spec_image
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.default_image)
    }

    /// What the job builders need: the resolved image plus the operator-wide
    /// pull policy.
    pub fn job_image<'a>(&'a self, spec_image: Option<&'a str>) -> JobImage<'a> {
        JobImage {
            image: self.resolve(spec_image),
            pull_policy: self.pull_policy.as_deref(),
        }
    }
}

/// The engine image a backup/restore Job container runs, as handed to the
/// job builders once the reconciler has resolved it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobImage<'a> {
    pub image: &'a str,
    /// `imagePullPolicy` for the container; `None` leaves the Kubernetes default.
    pub pull_policy: Option<&'a str>,
}

impl<'a> JobImage<'a> {
    pub fn new(image: &'a str, pull_policy: Option<&'a str>) -> Self {
        Self { image, pull_policy }
    }

    /// The release's compiled-in default with no pull policy — what a Job
    /// gets outside Helm when the resource does not pin an image.
    pub fn compiled_in() -> Self {
        Self::new(DEFAULT_BACKUP_IMAGE, None)
    }
}

/// Outcome of checking an image against [`MIN_SUPPORTED_ENGINE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineCheck {
    Supported(EngineVersion),
    Unsupported {
        found: EngineVersion,
        min: EngineVersion,
    },
    /// The tag does not name a release (`latest`, a digest, a custom tag):
    /// the policy cannot be applied, so the image is taken at face value.
    Unknown,
}

/// The release named by an image reference's tag, if any. A digest suffix
/// (`@sha256:…`) is ignored; a registry port (`host:5000/image`) is not
/// mistaken for a tag.
pub fn engine_version_of(image: &str) -> Option<EngineVersion> {
    let without_digest = image.split('@').next().unwrap_or(image);
    let (_, tag) = without_digest.rsplit_once(':')?;
    if tag.contains('/') {
        return None;
    }
    EngineVersion::parse(tag)
}

pub fn check_engine_version(image: &str) -> EngineCheck {
    match engine_version_of(image) {
        Some(found) if found < MIN_SUPPORTED_ENGINE => EngineCheck::Unsupported {
            found,
            min: MIN_SUPPORTED_ENGINE,
        },
        Some(found) => EngineCheck::Supported(found),
        None => EngineCheck::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn compiled_in_default_names_a_release() {
        assert!(
            engine_version_of(DEFAULT_BACKUP_IMAGE).is_some(),
            "DEFAULT_BACKUP_IMAGE must carry a vX.Y.Z tag"
        );
        assert_eq!(
            check_engine_version(DEFAULT_BACKUP_IMAGE),
            EngineCheck::Supported(engine_version_of(DEFAULT_BACKUP_IMAGE).unwrap())
        );
    }

    #[test]
    fn default_image_when_env_unset() {
        let config = EngineImageConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config, EngineImageConfig::compiled_in());
        assert_eq!(config.source, ImageSource::CompiledIn);
    }

    #[test]
    fn env_overrides_compiled_default() {
        let config = EngineImageConfig::from_lookup(lookup(&[(
            JOB_IMAGE_ENV,
            " osodevops/kafka-backup:v0.19.2 ",
        )]))
        .unwrap();
        assert_eq!(config.default_image, "osodevops/kafka-backup:v0.19.2");
        assert_eq!(config.source, ImageSource::Env);
    }

    #[test]
    fn empty_env_falls_back_to_default() {
        let config = EngineImageConfig::from_lookup(lookup(&[
            (JOB_IMAGE_ENV, "   "),
            (JOB_IMAGE_PULL_POLICY_ENV, ""),
        ]))
        .unwrap();
        assert_eq!(config, EngineImageConfig::compiled_in());
    }

    #[test]
    fn env_with_whitespace_is_invalid_config_naming_the_variable() {
        let err =
            EngineImageConfig::from_lookup(lookup(&[(JOB_IMAGE_ENV, "osodevops/kafka backup:v1")]))
                .unwrap_err();
        assert!(err.to_string().contains(JOB_IMAGE_ENV), "{err}");
    }

    #[test]
    fn pull_policy_accepts_k8s_values_only() {
        for policy in PULL_POLICIES {
            let config =
                EngineImageConfig::from_lookup(lookup(&[(JOB_IMAGE_PULL_POLICY_ENV, policy)]))
                    .unwrap();
            assert_eq!(config.pull_policy.as_deref(), Some(policy));
        }
        let err = EngineImageConfig::from_lookup(lookup(&[(JOB_IMAGE_PULL_POLICY_ENV, "always")]))
            .unwrap_err();
        assert!(err.to_string().contains(JOB_IMAGE_PULL_POLICY_ENV), "{err}");
    }

    #[test]
    fn resolve_prefers_spec_then_env_then_default() {
        let compiled = EngineImageConfig::compiled_in();
        assert_eq!(compiled.resolve(None), DEFAULT_BACKUP_IMAGE);
        assert_eq!(compiled.resolve(Some("  ")), DEFAULT_BACKUP_IMAGE);

        let from_env = EngineImageConfig::from_lookup(lookup(&[
            (JOB_IMAGE_ENV, "example/engine:v0.20.0"),
            (JOB_IMAGE_PULL_POLICY_ENV, "Always"),
        ]))
        .unwrap();
        assert_eq!(from_env.resolve(None), "example/engine:v0.20.0");
        assert_eq!(from_env.resolve(Some("mine:v0.21.0")), "mine:v0.21.0");
        assert_eq!(
            from_env.job_image(Some("mine:v0.21.0")),
            JobImage::new("mine:v0.21.0", Some("Always"))
        );
        assert_eq!(JobImage::compiled_in().image, DEFAULT_BACKUP_IMAGE);
    }

    #[test]
    fn parse_engine_version_plain_and_v_prefixed() {
        let expected = Some(EngineVersion {
            major: 0,
            minor: 19,
            patch: 1,
        });
        assert_eq!(engine_version_of("osodevops/kafka-backup:0.19.1"), expected);
        assert_eq!(
            engine_version_of("osodevops/kafka-backup:v0.19.1"),
            expected
        );
    }

    #[test]
    fn parse_engine_version_custom_registry_with_port() {
        assert_eq!(
            engine_version_of("registry.internal:5000/mirror/kafka-backup:v1.2.3"),
            Some(EngineVersion {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            engine_version_of("registry.internal:5000/mirror/kafka-backup"),
            None
        );
    }

    #[test]
    fn parse_engine_version_digest_latest_and_prerelease_are_unknown() {
        assert_eq!(
            engine_version_of("osodevops/kafka-backup@sha256:0123456789abcdef"),
            None
        );
        assert_eq!(
            engine_version_of("osodevops/kafka-backup:v0.19.1@sha256:0123456789abcdef"),
            Some(EngineVersion {
                major: 0,
                minor: 19,
                patch: 1
            }),
            "a tag alongside a digest still names the release"
        );
        assert_eq!(engine_version_of("osodevops/kafka-backup:latest"), None);
        assert_eq!(
            engine_version_of("osodevops/kafka-backup:v0.20.0-rc.1"),
            None
        );
        assert_eq!(engine_version_of("osodevops/kafka-backup:v0.20"), None);
        assert_eq!(engine_version_of("osodevops/kafka-backup"), None);
    }

    #[test]
    fn check_below_minimum_is_unsupported() {
        assert_eq!(
            check_engine_version("osodevops/kafka-backup:v0.15.13"),
            EngineCheck::Unsupported {
                found: EngineVersion {
                    major: 0,
                    minor: 15,
                    patch: 13
                },
                min: MIN_SUPPORTED_ENGINE,
            }
        );
    }

    #[test]
    fn check_equal_minimum_is_supported() {
        assert_eq!(
            check_engine_version("osodevops/kafka-backup:v0.16.0"),
            EngineCheck::Supported(MIN_SUPPORTED_ENGINE)
        );
        assert!(matches!(
            check_engine_version("osodevops/kafka-backup:v1.0.0"),
            EngineCheck::Supported(_)
        ));
    }

    #[test]
    fn check_unparseable_is_unknown() {
        assert_eq!(
            check_engine_version("osodevops/kafka-backup:latest"),
            EngineCheck::Unknown
        );
    }

    #[test]
    fn version_display_uses_v_prefix() {
        assert_eq!(MIN_SUPPORTED_ENGINE.to_string(), "v0.16.0");
    }
}
