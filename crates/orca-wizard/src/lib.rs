//! Orca setup wizard crate.

use std::fs;
use std::path::{Path, PathBuf};

use inquire::{Confirm, MultiSelect, Select, Text};
use orca_core::{Config, EffortPreset, MergeStrategy};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    pub force: bool,
    pub non_interactive: bool,
}

#[derive(Debug, Error)]
pub enum WizardError {
    #[error("failed to read/write wizard files: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("failed to load written config for validation: {0}")]
    ConfigLoad(#[from] orca_core::ConfigError),
    #[error("wizard prompt cancelled")]
    Cancelled,
}

/// Runs `orca init` flow and writes `.orca/config.toml` atomically.
pub fn run_init(project_dir: &Path, options: &InitOptions) -> Result<PathBuf, WizardError> {
    let orca_dir = project_dir.join(".orca");
    let config_path = orca_dir.join("config.toml");

    if config_path.exists() && !options.force {
        if options.non_interactive {
            return Err(WizardError::Cancelled);
        }
        let overwrite = Confirm::new(".orca/config.toml exists. Overwrite it?")
            .with_default(false)
            .prompt()
            .map_err(|_| WizardError::Cancelled)?;
        if !overwrite {
            return Err(WizardError::Cancelled);
        }
    }

    fs::create_dir_all(&orca_dir)?;
    let config = if options.non_interactive {
        Config::defaults()
    } else {
        prompt_config(project_dir)?
    };

    write_config_atomic(&config_path, &config)?;
    Ok(config_path)
}

fn prompt_config(project_dir: &Path) -> Result<Config, WizardError> {
    let mut config = Config::defaults();

    let default_name = project_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("orca-project")
        .to_owned();
    config.project.name = Text::new("Project name:")
        .with_default(&default_name)
        .prompt()
        .map_err(|_| WizardError::Cancelled)?;

    let agent_choices = vec![
        "claude-code".to_owned(),
        "codex".to_owned(),
        "gemini-cli".to_owned(),
        "pi".to_owned(),
        "opencode".to_owned(),
    ];
    let enabled_agents = MultiSelect::new("Enable agents:", agent_choices)
        .prompt()
        .map_err(|_| WizardError::Cancelled)?;
    apply_enabled_agents(&mut config, &enabled_agents);

    let preset = Select::new(
        "Default effort preset:",
        vec!["expensive", "quality", "balanced", "fast"],
    )
    .with_starting_cursor(1)
    .prompt()
    .map_err(|_| WizardError::Cancelled)?;
    config.effort.preset = match preset {
        "expensive" => EffortPreset::Expensive,
        "quality" => EffortPreset::Quality,
        "balanced" => EffortPreset::Balanced,
        _ => EffortPreset::Fast,
    };

    let merge = Select::new("Default merge strategy:", vec!["keep", "ff", "pr"])
        .prompt()
        .map_err(|_| WizardError::Cancelled)?;
    config.tasks.merge_strategy = match merge {
        "ff" => MergeStrategy::Ff,
        "pr" => MergeStrategy::Pr,
        _ => MergeStrategy::Keep,
    };

    Ok(config)
}

fn apply_enabled_agents(config: &mut Config, enabled: &[String]) {
    let has = |agent: &str| enabled.iter().any(|value| value == agent);
    config.agents.claude_code.enabled = has("claude-code");
    config.agents.codex.enabled = has("codex");
    config.agents.gemini_cli.enabled = has("gemini-cli");
    config.agents.pi.enabled = has("pi");
    config.agents.opencode.enabled = has("opencode");
}

fn write_config_atomic(path: &Path, config: &Config) -> Result<(), WizardError> {
    let serialized = toml::to_string_pretty(config)?;
    let temp_path = path.with_extension("toml.tmp");
    fs::write(&temp_path, serialized)?;
    fs::rename(&temp_path, path)?;
    let _ = Config::load_from(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{InitOptions, run_init};

    #[test]
    fn non_interactive_init_writes_valid_config() {
        let root = std::env::temp_dir().join(format!(
            "orca-wizard-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp root should be created");

        let path = run_init(
            &root,
            &InitOptions {
                force: false,
                non_interactive: true,
            },
        )
        .expect("init should write config");
        assert!(path.exists());
        let loaded = orca_core::Config::load_from(&path).expect("written config should load");
        assert_eq!(loaded.project.default_branch, "main");
    }

    #[test]
    fn force_overwrites_existing_config() {
        let root = std::env::temp_dir().join(format!(
            "orca-wizard-test-overwrite-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        let orca_dir = root.join(".orca");
        std::fs::create_dir_all(&orca_dir).expect("orca dir should be created");
        let config_path = orca_dir.join("config.toml");
        std::fs::write(&config_path, "broken").expect("broken config should be written");

        run_init(
            &root,
            &InitOptions {
                force: true,
                non_interactive: true,
            },
        )
        .expect("force init should overwrite");

        let loaded = orca_core::Config::load_from(&config_path)
            .expect("overwritten config should deserialize");
        assert_eq!(loaded.orca.socket, "state/daemon.sock");
    }
}
