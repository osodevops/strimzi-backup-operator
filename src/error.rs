#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Strimzi cluster '{name}' not found in namespace '{namespace}'")]
    StrimziClusterNotFound { name: String, namespace: String },

    #[error("Secret '{name}' not found in namespace '{namespace}'")]
    SecretNotFound { name: String, namespace: String },

    #[error("Secret '{name}' missing key '{key}'")]
    SecretKeyMissing { name: String, key: String },

    #[error("KafkaUser '{name}' not found in namespace '{namespace}'")]
    KafkaUserNotFound { name: String, namespace: String },

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Backup '{name}' not found")]
    BackupNotFound { name: String },

    #[error("Job creation failed: {0}")]
    JobCreationFailed(String),

    #[error("Finalizer error: {0}")]
    Finalizer(String),

    #[error("Missing object key: {0}")]
    MissingObjectKey(&'static str),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn reason(&self) -> &str {
        match self {
            Error::Kube(_) => "KubernetesError",
            Error::Serialization(_) => "SerializationError",
            Error::Yaml(_) => "SerializationError",
            Error::StrimziClusterNotFound { .. } => "StrimziClusterNotFound",
            Error::SecretNotFound { .. } => "SecretNotFound",
            Error::SecretKeyMissing { .. } => "SecretKeyMissing",
            Error::KafkaUserNotFound { .. } => "KafkaUserNotFound",
            Error::InvalidConfig(_) => "InvalidConfiguration",
            Error::BackupNotFound { .. } => "BackupNotFound",
            Error::JobCreationFailed(_) => "JobCreationFailed",
            Error::Finalizer(_) => "FinalizerError",
            Error::MissingObjectKey(_) => "MissingObjectKey",
            Error::Regex(_) => "RegexError",
        }
    }
}
