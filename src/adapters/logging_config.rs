use serde_yaml::Value;

use crate::crd::common::LoggingSpec;

/// Build the kafka-backup logging config section.
pub fn build_logging_config(logging: &LoggingSpec) -> Value {
    let mut config = serde_yaml::Mapping::new();

    if let Some(level) = &logging.level {
        config.insert(
            Value::String("level".to_string()),
            Value::String(level.clone()),
        );
    }
    if let Some(format) = &logging.format {
        config.insert(
            Value::String("format".to_string()),
            Value::String(format.clone()),
        );
    }
    if let Some(output) = &logging.output {
        config.insert(
            Value::String("output".to_string()),
            Value::String(output.clone()),
        );
    }
    if !logging.modules.is_empty() {
        config.insert(
            Value::String("modules".to_string()),
            Value::Mapping(
                logging
                    .modules
                    .iter()
                    .map(|(module, level)| {
                        (Value::String(module.clone()), Value::String(level.clone()))
                    })
                    .collect(),
            ),
        );
    }
    if let Some(rotation) = &logging.rotation {
        let mut rotation_config = serde_yaml::Mapping::new();
        if let Some(max_size_mb) = rotation.max_size_mb {
            rotation_config.insert(
                Value::String("max_size_mb".to_string()),
                Value::Number(serde_yaml::Number::from(max_size_mb)),
            );
        }
        if let Some(max_files) = rotation.max_files {
            rotation_config.insert(
                Value::String("max_files".to_string()),
                Value::Number(serde_yaml::Number::from(max_files)),
            );
        }
        if !rotation_config.is_empty() {
            config.insert(
                Value::String("rotation".to_string()),
                Value::Mapping(rotation_config),
            );
        }
    }

    Value::Mapping(config)
}
