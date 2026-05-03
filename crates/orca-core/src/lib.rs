//! Core Orca domain types and invariants.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level Orca configuration schema represented by `.orca/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    pub orca: OrcaConfig,
    pub effort: EffortConfig,
    pub agents: AgentsConfig,
    pub routing: RoutingConfig,
    pub skills: SkillsConfig,
    pub tui: TuiConfig,
    pub tasks: TasksConfig,
}

impl Config {
    /// Returns default configuration values for new Orca projects.
    pub fn defaults() -> Self {
        Self {
            project: ProjectConfig {
                name: "orca-project".to_owned(),
                root: ".".to_owned(),
                default_branch: "main".to_owned(),
            },
            orca: OrcaConfig {
                socket: "state/daemon.sock".to_owned(),
                worktrees_dir: "worktrees".to_owned(),
                event_log_retention: 10,
            },
            effort: EffortConfig {
                preset: EffortPreset::Quality,
            },
            agents: AgentsConfig {
                claude_code: AgentConfig {
                    enabled: true,
                    command: "claude".to_owned(),
                    models: BTreeMap::from([
                        ("planning".to_owned(), "claude-opus-4-7".to_owned()),
                        ("implementing".to_owned(), "claude-sonnet-4-6".to_owned()),
                        ("reviewing".to_owned(), "claude-sonnet-4-6".to_owned()),
                    ]),
                    daily_usage_limit_usd: 20.0,
                    extra_args: Vec::new(),
                },
                codex: AgentConfig {
                    enabled: true,
                    command: "codex".to_owned(),
                    models: BTreeMap::from([
                        ("planning".to_owned(), "o3".to_owned()),
                        ("implementing".to_owned(), "o3-mini".to_owned()),
                        ("reviewing".to_owned(), "o3".to_owned()),
                    ]),
                    daily_usage_limit_usd: 10.0,
                    extra_args: Vec::new(),
                },
                gemini_cli: AgentConfig {
                    enabled: true,
                    command: "gemini".to_owned(),
                    models: BTreeMap::from([
                        ("planning".to_owned(), "gemini-2.5-pro".to_owned()),
                        ("implementing".to_owned(), "gemini-2.5-flash".to_owned()),
                        ("reviewing".to_owned(), "gemini-2.5-pro".to_owned()),
                    ]),
                    daily_usage_limit_usd: 10.0,
                    extra_args: Vec::new(),
                },
                pi: AgentConfig {
                    enabled: true,
                    command: "pi".to_owned(),
                    models: BTreeMap::from([("default".to_owned(), "kimi-k2.5:cloud".to_owned())]),
                    daily_usage_limit_usd: 5.0,
                    extra_args: vec!["--rpc".to_owned()],
                },
                opencode: AgentConfig {
                    enabled: true,
                    command: "opencode".to_owned(),
                    models: BTreeMap::from([(
                        "default".to_owned(),
                        "claude-sonnet-4-6".to_owned(),
                    )]),
                    daily_usage_limit_usd: 10.0,
                    extra_args: Vec::new(),
                },
            },
            routing: RoutingConfig {
                rules: vec![
                    RoutingRule {
                        needs: vec!["long_context".to_owned()],
                        prefer: "gemini-cli".to_owned(),
                    },
                    RoutingRule {
                        needs: vec!["adversarial_review".to_owned()],
                        prefer: "codex".to_owned(),
                    },
                    RoutingRule {
                        needs: vec!["rpc_driven".to_owned()],
                        prefer: "pi".to_owned(),
                    },
                    RoutingRule {
                        needs: vec!["local_model".to_owned()],
                        prefer: "opencode".to_owned(),
                    },
                    RoutingRule {
                        needs: vec!["multi_file_edit".to_owned()],
                        prefer: "claude-code".to_owned(),
                    },
                ],
                default: "claude-code".to_owned(),
                auto_accept: false,
            },
            skills: SkillsConfig {
                graphify: SkillToggle {
                    enabled: true,
                    mcp: Some(true),
                },
                caveman: SkillToggle {
                    enabled: false,
                    mcp: None,
                },
                custom: None,
            },
            tui: TuiConfig {
                refresh_hz: 10,
                theme: "default".to_owned(),
                show_cost: true,
                show_tokens: true,
            },
            tasks: TasksConfig {
                merge_strategy: MergeStrategy::Keep,
            },
        }
    }

    /// Loads configuration from TOML file, merging provided values with defaults.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let mut value: toml::Value = toml::from_str(&content).map_err(ConfigError::TomlParse)?;
        validate_agent_ids(&value)?;
        expand_env_in_value(&mut value, String::new())?;

        let partial: PartialConfig = value
            .try_into()
            .map_err(|source| ConfigError::TomlDecode { source })?;
        let mut config = Self::defaults();
        config.apply_partial(partial);
        config.validate()?;
        Ok(config)
    }

    fn apply_partial(&mut self, partial: PartialConfig) {
        if let Some(project) = partial.project {
            if let Some(name) = project.name {
                self.project.name = name;
            }
            if let Some(root) = project.root {
                self.project.root = root;
            }
            if let Some(default_branch) = project.default_branch {
                self.project.default_branch = default_branch;
            }
        }

        if let Some(orca) = partial.orca {
            if let Some(socket) = orca.socket {
                self.orca.socket = socket;
            }
            if let Some(worktrees_dir) = orca.worktrees_dir {
                self.orca.worktrees_dir = worktrees_dir;
            }
            if let Some(event_log_retention) = orca.event_log_retention {
                self.orca.event_log_retention = event_log_retention;
            }
        }

        if let Some(effort) = partial.effort
            && let Some(preset) = effort.preset
        {
            self.effort.preset = preset;
        }

        if let Some(agents) = partial.agents {
            apply_partial_agent(&mut self.agents.claude_code, agents.claude_code);
            apply_partial_agent(&mut self.agents.codex, agents.codex);
            apply_partial_agent(&mut self.agents.gemini_cli, agents.gemini_cli);
            apply_partial_agent(&mut self.agents.pi, agents.pi);
            apply_partial_agent(&mut self.agents.opencode, agents.opencode);
        }

        if let Some(routing) = partial.routing {
            if let Some(rules) = routing.rules {
                self.routing.rules = rules;
            }
            if let Some(default) = routing.default {
                self.routing.default = default;
            }
            if let Some(auto_accept) = routing.auto_accept {
                self.routing.auto_accept = auto_accept;
            }
        }

        if let Some(skills) = partial.skills {
            apply_partial_skill(&mut self.skills.graphify, skills.graphify);
            apply_partial_skill(&mut self.skills.caveman, skills.caveman);
            if let Some(custom) = skills.custom {
                self.skills.custom = Some(custom);
            }
        }

        if let Some(tui) = partial.tui {
            if let Some(refresh_hz) = tui.refresh_hz {
                self.tui.refresh_hz = refresh_hz;
            }
            if let Some(theme) = tui.theme {
                self.tui.theme = theme;
            }
            if let Some(show_cost) = tui.show_cost {
                self.tui.show_cost = show_cost;
            }
            if let Some(show_tokens) = tui.show_tokens {
                self.tui.show_tokens = show_tokens;
            }
        }

        if let Some(tasks) = partial.tasks
            && let Some(merge_strategy) = tasks.merge_strategy
        {
            self.tasks.merge_strategy = merge_strategy;
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for (agent_id, agent) in [
            ("claude-code", &self.agents.claude_code),
            ("codex", &self.agents.codex),
            ("gemini-cli", &self.agents.gemini_cli),
            ("pi", &self.agents.pi),
            ("opencode", &self.agents.opencode),
        ] {
            if agent.daily_usage_limit_usd < 0.0 {
                return Err(ConfigError::InvalidField {
                    field: format!("agents.{agent_id}.daily_usage_limit_usd"),
                    message: "must be non-negative".to_owned(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse TOML syntax: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("failed to decode config schema: {source}")]
    TomlDecode { source: toml::de::Error },
    #[error("unknown agent id `{agent_id}` at `agents.{agent_id}`")]
    UnknownAgentId { agent_id: String },
    #[error("missing environment variable `{var}` for field `{field}`")]
    MissingEnvVar { var: String, field: String },
    #[error("invalid value for `{field}`: {message}")]
    InvalidField { field: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialConfig {
    project: Option<PartialProjectConfig>,
    orca: Option<PartialOrcaConfig>,
    effort: Option<PartialEffortConfig>,
    agents: Option<PartialAgentsConfig>,
    routing: Option<PartialRoutingConfig>,
    skills: Option<PartialSkillsConfig>,
    tui: Option<PartialTuiConfig>,
    tasks: Option<PartialTasksConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialProjectConfig {
    name: Option<String>,
    root: Option<String>,
    default_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialOrcaConfig {
    socket: Option<String>,
    worktrees_dir: Option<String>,
    event_log_retention: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialEffortConfig {
    preset: Option<EffortPreset>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialAgentsConfig {
    #[serde(rename = "claude-code")]
    claude_code: Option<PartialAgentConfig>,
    codex: Option<PartialAgentConfig>,
    #[serde(rename = "gemini-cli")]
    gemini_cli: Option<PartialAgentConfig>,
    pi: Option<PartialAgentConfig>,
    opencode: Option<PartialAgentConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialAgentConfig {
    enabled: Option<bool>,
    command: Option<String>,
    models: Option<BTreeMap<String, String>>,
    daily_usage_limit_usd: Option<f64>,
    extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialRoutingConfig {
    rules: Option<Vec<RoutingRule>>,
    default: Option<String>,
    auto_accept: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialSkillsConfig {
    graphify: Option<PartialSkillToggle>,
    caveman: Option<PartialSkillToggle>,
    custom: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialSkillToggle {
    enabled: Option<bool>,
    mcp: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialTuiConfig {
    refresh_hz: Option<u16>,
    theme: Option<String>,
    show_cost: Option<bool>,
    show_tokens: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PartialTasksConfig {
    merge_strategy: Option<MergeStrategy>,
}

fn apply_partial_agent(target: &mut AgentConfig, partial: Option<PartialAgentConfig>) {
    if let Some(partial) = partial {
        if let Some(enabled) = partial.enabled {
            target.enabled = enabled;
        }
        if let Some(command) = partial.command {
            target.command = command;
        }
        if let Some(models) = partial.models {
            target.models = models;
        }
        if let Some(daily_usage_limit_usd) = partial.daily_usage_limit_usd {
            target.daily_usage_limit_usd = daily_usage_limit_usd;
        }
        if let Some(extra_args) = partial.extra_args {
            target.extra_args = extra_args;
        }
    }
}

fn apply_partial_skill(target: &mut SkillToggle, partial: Option<PartialSkillToggle>) {
    if let Some(partial) = partial {
        if let Some(enabled) = partial.enabled {
            target.enabled = enabled;
        }
        if let Some(mcp) = partial.mcp {
            target.mcp = Some(mcp);
        }
    }
}

fn validate_agent_ids(value: &toml::Value) -> Result<(), ConfigError> {
    let allowed = ["claude-code", "codex", "gemini-cli", "pi", "opencode"];
    let Some(agents_table) = value.get("agents").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    for key in agents_table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ConfigError::UnknownAgentId {
                agent_id: key.clone(),
            });
        }
    }
    Ok(())
}

fn expand_env_in_value(value: &mut toml::Value, field_path: String) -> Result<(), ConfigError> {
    match value {
        toml::Value::String(s) => {
            let expanded = expand_env_string(s, &field_path)?;
            *s = expanded;
            Ok(())
        }
        toml::Value::Array(arr) => {
            for (i, item) in arr.iter_mut().enumerate() {
                let next_path = format!("{field_path}[{i}]");
                expand_env_in_value(item, next_path)?;
            }
            Ok(())
        }
        toml::Value::Table(table) => {
            for (key, item) in table.iter_mut() {
                let next_path = if field_path.is_empty() {
                    key.clone()
                } else {
                    format!("{field_path}.{key}")
                };
                expand_env_in_value(item, next_path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn expand_env_string(input: &str, field: &str) -> Result<String, ConfigError> {
    let mut output = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next();
            let mut var = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                var.push(next);
            }
            if var.is_empty() {
                continue;
            }
            let value = std::env::var(&var).map_err(|_| ConfigError::MissingEnvVar {
                var: var.clone(),
                field: field.to_owned(),
            })?;
            output.push_str(&value);
        } else {
            output.push(ch);
        }
    }

    Ok(output)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub root: String,
    pub default_branch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrcaConfig {
    pub socket: String,
    pub worktrees_dir: String,
    pub event_log_retention: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffortConfig {
    pub preset: EffortPreset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortPreset {
    Expensive,
    Quality,
    Balanced,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(rename = "claude-code")]
    pub claude_code: AgentConfig,
    pub codex: AgentConfig,
    #[serde(rename = "gemini-cli")]
    pub gemini_cli: AgentConfig,
    pub pi: AgentConfig,
    pub opencode: AgentConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    pub command: String,
    pub models: BTreeMap<String, String>,
    pub daily_usage_limit_usd: f64,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingConfig {
    pub rules: Vec<RoutingRule>,
    pub default: String,
    pub auto_accept: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingRule {
    pub needs: Vec<String>,
    pub prefer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillsConfig {
    pub graphify: SkillToggle,
    pub caveman: SkillToggle,
    pub custom: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillToggle {
    pub enabled: bool,
    pub mcp: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuiConfig {
    pub refresh_hz: u16,
    pub theme: String,
    pub show_cost: bool,
    pub show_tokens: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasksConfig {
    pub merge_strategy: MergeStrategy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    Ff,
    Pr,
    Keep,
}

/// Task identifier format `T-NNN`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskError> {
        let value = value.into();
        let valid = value.starts_with("T-")
            && value.len() >= 4
            && value.as_bytes()[2..]
                .iter()
                .all(|byte| byte.is_ascii_digit());
        if !valid {
            return Err(TaskError::InvalidTaskId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// On-disk task representation for `.orca/state/tasks/<id>/task.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub state: TaskState,
    pub created_at: String,
    pub updated_at: String,
    pub assigned_to: Option<String>,
    pub reviewer: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub parent: Option<TaskId>,
    #[serde(default)]
    pub subtasks: Vec<TaskId>,
    #[serde(default)]
    pub context_files: Vec<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl Task {
    /// Reads and deserializes a task from a `task.toml` file.
    pub fn from_file(path: &Path) -> Result<Self, TaskError> {
        let content = fs::read_to_string(path)?;
        let task: Self = toml::from_str(&content).map_err(TaskError::TomlParse)?;
        Ok(task)
    }

    /// Writes a task to disk atomically (temp file + rename).
    pub fn write_atomic(&self, path: &Path) -> Result<(), TaskError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self).map_err(TaskError::TomlSerialize)?;
        let temp_path = path.with_extension("toml.tmp");
        fs::write(&temp_path, serialized)?;
        fs::rename(&temp_path, path)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Drafted,
    Planning,
    Planned,
    Implementing,
    Implemented,
    Reviewing,
    Reviewed,
    Revising,
    Blocked,
    Parked,
    Done,
}

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("invalid task id `{0}`; expected format T-NNN")]
    InvalidTaskId(String),
    #[error("failed to read or write task file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse task TOML: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("failed to serialize task TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RpcRequest {
    Ping,
    CreateTask { task: Task },
    GetTask { id: TaskId },
    ListTasks,
    UpdateTaskState { id: TaskId, state: TaskState },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RpcResponse {
    Pong,
    TaskCreated { task: Task },
    Task { task: Task },
    TaskList { tasks: Vec<Task> },
    TaskStateUpdated { id: TaskId, state: TaskState },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        Config, ConfigError, MergeStrategy, RpcRequest, RpcResponse, Task, TaskId, TaskState,
    };

    const STATE_MD_EXAMPLE: &str = r#"
[project]
name = "liat-ball-detection"
root = "."
default_branch = "main"

[orca]
socket = "state/daemon.sock"
worktrees_dir = "worktrees"
event_log_retention = 10

[effort]
preset = "quality"

[agents.claude-code]
enabled = true
command = "claude"
models = { planning = "claude-opus-4-7", implementing = "claude-sonnet-4-6", reviewing = "claude-sonnet-4-6" }
daily_usage_limit_usd = 20.0
extra_args = []

[agents.codex]
enabled = true
command = "codex"
models = { planning = "o3", implementing = "o3-mini", reviewing = "o3" }
daily_usage_limit_usd = 10.0

[agents.gemini-cli]
enabled = true
command = "gemini"
models = { planning = "gemini-2.5-pro", implementing = "gemini-2.5-flash", reviewing = "gemini-2.5-pro" }
daily_usage_limit_usd = 10.0

[agents.pi]
enabled = true
command = "pi"
extra_args = ["--rpc"]
models = { default = "kimi-k2.5:cloud" }
daily_usage_limit_usd = 5.0

[agents.opencode]
enabled = true
command = "opencode"
models = { default = "claude-sonnet-4-6" }
daily_usage_limit_usd = 10.0

[routing]
rules = [
  { needs = ["long_context"], prefer = "gemini-cli" },
  { needs = ["adversarial_review"], prefer = "codex" },
  { needs = ["rpc_driven"], prefer = "pi" },
  { needs = ["local_model"], prefer = "opencode" },
  { needs = ["multi_file_edit"], prefer = "claude-code" },
]
default = "claude-code"
auto_accept = false

[skills]
graphify = { enabled = true, mcp = true }
caveman = { enabled = false }

[tui]
refresh_hz = 10
theme = "default"
show_cost = true
show_tokens = true

[tasks]
merge_strategy = "keep"
"#;

    #[test]
    fn config_round_trip_serialization() {
        let original = Config::defaults();
        let serialized = toml::to_string(&original).expect("defaults should serialize");
        let parsed: Config = toml::from_str(&serialized).expect("serialized config should parse");
        assert_eq!(parsed, original);
    }

    #[test]
    fn state_md_example_parses() {
        let parsed: Config =
            toml::from_str(STATE_MD_EXAMPLE).expect("STATE.md config example should parse");
        assert_eq!(parsed.project.name, "liat-ball-detection");
        assert_eq!(parsed.routing.default, "claude-code");
        assert!(!parsed.routing.auto_accept);
    }

    #[test]
    fn defaults_produce_parseable_toml() {
        let serialized =
            toml::to_string_pretty(&Config::defaults()).expect("defaults should serialize to TOML");
        let parsed: Config = toml::from_str(&serialized).expect("serialized defaults should parse");
        assert_eq!(parsed.tasks.merge_strategy, MergeStrategy::Keep);
    }

    #[test]
    fn load_from_merges_partial_with_defaults() {
        let path = write_temp_config(
            r#"
[project]
name = "my-project"

[agents.codex]
enabled = false
daily_usage_limit_usd = 2.5
"#,
        );
        let loaded = Config::load_from(&path).expect("partial config should load");
        assert_eq!(loaded.project.name, "my-project");
        assert_eq!(loaded.project.default_branch, "main");
        assert!(!loaded.agents.codex.enabled);
        assert_eq!(loaded.agents.codex.daily_usage_limit_usd, 2.5);
    }

    #[test]
    fn load_from_rejects_unknown_agent_id() {
        let path = write_temp_config(
            r#"
[agents.foobar]
enabled = true
"#,
        );
        let err = Config::load_from(&path).expect_err("unknown agent should fail");
        let message = err.to_string();
        assert!(matches!(err, ConfigError::UnknownAgentId { .. }));
        assert!(message.contains("agents.foobar"));
    }

    #[test]
    fn load_from_rejects_negative_usage_limits() {
        let path = write_temp_config(
            r#"
[agents.codex]
daily_usage_limit_usd = -1.0
"#,
        );
        let err = Config::load_from(&path).expect_err("negative usage limit should fail");
        let message = err.to_string();
        assert!(matches!(err, ConfigError::InvalidField { .. }));
        assert!(message.contains("agents.codex.daily_usage_limit_usd"));
    }

    #[test]
    fn load_from_expands_environment_variables() {
        let path = write_temp_config(
            r#"
[project]
root = "${ORCA_TEST_ROOT}"
"#,
        );

        // SAFETY: tests in this crate mutate a dedicated variable for this test.
        unsafe { std::env::set_var("ORCA_TEST_ROOT", "/tmp/orca-root") };

        let loaded = Config::load_from(&path).expect("env var should expand");
        assert_eq!(loaded.project.root, "/tmp/orca-root");
    }

    #[test]
    fn load_from_errors_on_missing_environment_variable() {
        let path = write_temp_config(
            r#"
[project]
root = "${ORCA_MISSING_VAR}"
"#,
        );

        // SAFETY: tests in this crate mutate a dedicated variable for this test.
        unsafe { std::env::remove_var("ORCA_MISSING_VAR") };

        let err = Config::load_from(&path).expect_err("missing env var should fail");
        assert!(matches!(err, ConfigError::MissingEnvVar { .. }));
        assert!(err.to_string().contains("ORCA_MISSING_VAR"));
    }

    fn write_temp_config(content: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("orca-config-test-{nanos}.toml"));
        std::fs::write(&path, content).expect("temp config should be writable");
        path
    }

    #[test]
    fn task_round_trip_write_then_read() {
        let task = Task {
            id: TaskId::new("T-007").expect("task id should be valid"),
            title: "Fix referee false-positive in dark-kit frames".to_owned(),
            description: "Detailed task description".to_owned(),
            state: TaskState::Implementing,
            created_at: "2026-04-22T09:14:00Z".to_owned(),
            updated_at: "2026-04-22T10:02:11Z".to_owned(),
            assigned_to: Some("claude-code".to_owned()),
            reviewer: Some("codex".to_owned()),
            capabilities: vec![
                "multi_file_edit".to_owned(),
                "domain_specific".to_owned(),
                "needs_review".to_owned(),
            ],
            parent: None,
            subtasks: Vec::new(),
            context_files: vec![
                "src/card_classifier/model.py".to_owned(),
                "src/card_classifier/train.py".to_owned(),
            ],
            worktree: Some("../../../worktrees/T-007".to_owned()),
            branch: Some("orca/T-007".to_owned()),
            acceptance: vec![
                "False-positive rate reduced to < 0.5%".to_owned(),
                "No regression on existing test set".to_owned(),
            ],
            notes: "— 10:02 LB: plan approved".to_owned(),
        };

        let path = std::env::temp_dir().join(format!(
            "orca-task-roundtrip-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        task.write_atomic(&path).expect("task should be written");
        let read_back = Task::from_file(&path).expect("task should read back");
        assert_eq!(read_back, task);
    }

    #[test]
    fn hand_edited_task_toml_parses() {
        let path = std::env::temp_dir().join(format!(
            "orca-task-handedit-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        let content = r#"
id = "T-042"
title = "Investigate card-classifier drift"
description = "Short description"
state = "drafted"
created_at = "2026-04-22T09:14:00Z"
updated_at = "2026-04-22T09:14:00Z"
capabilities = ["surgical_edit"]
subtasks = []
context_files = []
acceptance = ["Preserve current metrics"]
notes = ""
"#;
        std::fs::write(&path, content).expect("task file should be writable");
        let parsed = Task::from_file(&path).expect("hand-edited task should parse");
        assert_eq!(parsed.id.as_str(), "T-042");
        assert_eq!(parsed.state, TaskState::Drafted);
        assert_eq!(parsed.capabilities, vec!["surgical_edit".to_owned()]);
    }

    #[test]
    fn rpc_request_serializes_to_tagged_shape() {
        let request = RpcRequest::ListTasks;
        let value = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(value["type"], "ListTasks");
    }

    #[test]
    fn rpc_response_serializes_to_tagged_shape() {
        let response = RpcResponse::Pong;
        let value = serde_json::to_value(&response).expect("response should serialize");
        assert_eq!(value["type"], "Pong");
    }
}
