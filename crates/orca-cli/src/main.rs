use clap::{Parser, Subcommand};
use orca_daemon::{DaemonOptions, run_daemon};
use orca_wizard::{InitOptions, run_init};
use std::fs::{self, OpenOptions};
use std::io::stderr;
use std::path::{Path, PathBuf};
use tracing_subscriber::{EnvFilter, fmt};

mod daemon_client;

const NOT_YET_IMPLEMENTED: &str = "not yet implemented";

#[derive(Debug, Parser)]
#[command(name = "orca")]
#[command(about = "A tmux-first, agent-agnostic orchestrator for AI coding agents.")]
struct Cli {
    #[arg(long, value_name = "DIR")]
    project: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(short, long, action = clap::ArgAction::Count)]
    quiet: u8,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(InitArgs),
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Daemon(DaemonArgs),
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    Kb {
        #[command(subcommand)]
        command: KbCommand,
    },
    Status,
    Watch,
    Ping,
    Version,
    Doctor,
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    #[arg(long)]
    force: bool,
    #[arg(long)]
    non_interactive: bool,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Edit,
    Set { path: String, value: String },
    Get { path: String },
    Show,
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    Stop,
    Restart,
}

#[derive(Debug, clap::Args)]
struct DaemonArgs {
    #[arg(short = 'f', long = "foreground")]
    foreground: bool,
    #[arg(long, value_name = "PATH")]
    pid_file: Option<String>,
    #[arg(long, value_name = "PATH")]
    socket: Option<String>,
    #[command(subcommand)]
    command: Option<DaemonCommand>,
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    New { title: Option<String> },
    List,
    Show { id: String },
    Route { id: String, to: Option<String> },
    Start { id: String },
    Pause { id: String },
    Review { id: String, by: Option<String> },
    Accept { id: String },
    Revise { id: String },
    Cancel { id: String },
    Cleanup { id: String },
    Tree { id: Option<String> },
    Export { id: String, format: Option<String> },
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    List,
    Status { id: String },
    Kill { id: String },
    Reset { id: String },
    Test { id: String },
}

#[derive(Debug, Subcommand)]
enum KbCommand {
    Init,
    Update,
    Query {
        text: String,
    },
    Path {
        from: String,
        to: String,
    },
    Explain,
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    Start,
    Stop,
}

fn version_string() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() {
    let cli = Cli::parse();
    init_logging(&cli).unwrap_or_else(|error| exit_with_error(error));

    match &cli.command {
        Some(Command::Version) => println!("orca {}", version_string()),
        Some(Command::Daemon(args)) => run_daemon_command(&cli, args),
        Some(Command::Init(args)) => run_init_command(&cli, args),
        Some(Command::Ping) => run_ping_command(&cli),
        _ => {
            eprintln!("{NOT_YET_IMPLEMENTED}");
            std::process::exit(1);
        }
    }
}

fn run_daemon_command(cli: &Cli, args: &DaemonArgs) {
    if args.command.is_some() {
        eprintln!("{NOT_YET_IMPLEMENTED}");
        std::process::exit(1);
    }

    if args.pid_file.is_some() || args.socket.is_some() {
        eprintln!("{NOT_YET_IMPLEMENTED}");
        std::process::exit(1);
    }

    if args.foreground {
        let mut options = DaemonOptions::from_current_dir().unwrap_or_else(|error| {
            exit_with_error(error);
        });
        if let Some(project) = &cli.project {
            options.project_dir = std::path::PathBuf::from(project);
        }
        run_daemon(&options).unwrap_or_else(|error| exit_with_error(error));
        return;
    }

    let mut command = std::process::Command::new(
        std::env::current_exe().unwrap_or_else(|err| exit_with_error(err)),
    );
    if let Some(project) = &cli.project {
        command.arg("--project").arg(project);
    }
    let project_dir = cli
        .project
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|err| exit_with_error(err)));
    let log_path = project_dir.join(".orca/logs/daemon.log");
    command.env("ORCA_DAEMONIZED", "1");
    command.env("ORCA_DAEMON_LOG_FILE", log_path);
    command.arg("daemon").arg("--foreground");
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());

    command
        .spawn()
        .unwrap_or_else(|error| exit_with_error(error));
}

fn exit_with_error<E: std::fmt::Display>(error: E) -> ! {
    eprintln!("{error}");
    std::process::exit(1);
}

fn run_ping_command(cli: &Cli) {
    let project_dir = cli.project.as_deref().map(Path::new);
    let response = daemon_client::request(project_dir, r#"{"type":"ping"}"#)
        .unwrap_or_else(|error| exit_with_error(error));
    println!("{response}");
}

fn run_init_command(cli: &Cli, args: &InitArgs) {
    let project_dir = cli
        .project
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|error| exit_with_error(error)));
    let options = InitOptions {
        force: args.force,
        non_interactive: args.non_interactive,
    };
    let written = run_init(&project_dir, &options).unwrap_or_else(|error| exit_with_error(error));
    println!("wrote {}", written.display());
}

fn init_logging(cli: &Cli) -> std::io::Result<()> {
    let default_directive = default_directive(cli);
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    let daemon_log_path = std::env::var_os("ORCA_DAEMON_LOG_FILE");
    if daemon_log_path.is_some() && std::env::var_os("ORCA_DAEMONIZED").is_some() {
        let log_path = std::path::PathBuf::from(daemon_log_path.unwrap_or_default());
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        fmt()
            .with_ansi(false)
            .with_writer(move || {
                file.try_clone()
                    .expect("failed to clone daemon log file handle")
            })
            .with_env_filter(env_filter)
            .init();
    } else {
        fmt().with_writer(stderr).with_env_filter(env_filter).init();
    }
    Ok(())
}

fn default_directive(cli: &Cli) -> &'static str {
    if cli.quiet > 0 {
        return "error";
    }
    match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    }
}
