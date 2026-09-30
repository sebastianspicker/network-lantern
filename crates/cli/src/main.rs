use clap::{Args, Parser, Subcommand};
use lantern_contracts::{Error, ErrorCategory, Result};
use lantern_platform::{PROFILE_LIMIT, read_json};
use lantern_runtime::{profiles::ProfileStore, reports};
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "network-lantern",
    version,
    about = "Native network measurement, planning and saved-run inspection",
    long_about = "Network Lantern keeps planning separate from execution. --dry-run never resolves DNS, connects sockets, authorizes a helper or writes results. Throughput statuses: 0 success, 11 validation, 12 prerequisite, 13 connectivity, 14 partial failure, 15 total failure, 16 internal; interruption 130/143."
)]
struct Cli {
    /// Emit compact machine-readable JSON, including errors.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Inspect local platform and engine availability without probes.
    Doctor,
    /// Basic diagnostics or the independent MTR-style trace suite.
    Path {
        #[command(subcommand)]
        command: PathCommand,
    },
    /// Native iperf3-compatible throughput suite.
    Throughput(Measurement),
    /// Ordered capability workflow; planning never executes children.
    Workflow {
        #[arg(value_enum)]
        name: Option<WorkflowName>,
        #[command(flatten)]
        options: Measurement,
    },
    /// Explicit local profile operations.
    Profiles {
        #[arg(long, default_value = ".iperf3/profiles.json")]
        store: PathBuf,
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Inspect, compare and export saved evidence.
    Runs {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Windows tuning with verified backup and recovery.
    Tuning(Measurement),
    /// Privileged helper registration and status.
    Helper {
        #[command(subcommand)]
        command: HelperCommand,
    },
}
#[derive(Subcommand)]
enum PathCommand {
    Basic(Measurement),
    Trace(Measurement),
}
#[derive(clap::ValueEnum, Clone)]
enum WorkflowName {
    Triage,
    Path,
    Throughput,
    Baseline,
    WindowsTuning,
}
#[derive(Args)]
struct Measurement {
    /// Validate and print the complete plan without network or output writes.
    #[arg(long)]
    dry_run: bool,
    /// JSON configuration applied after the named profile.
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long, default_value = ".iperf3/profiles.json")]
    profiles_file: PathBuf,
    /// Reject unknown configuration keys.
    #[arg(long)]
    strict: bool,
    /// Final JSON override layer; repeated matrix entries are preserved.
    #[arg(long, default_value = "{}")]
    settings: String,
    /// Throughput server or path host; resolved only during execution.
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    /// Measured seconds per throughput test, excluding omission.
    #[arg(long)]
    duration: Option<u64>,
    #[arg(long)]
    omit: Option<u64>,
    #[arg(long)]
    protocol: Option<String>,
    #[arg(long)]
    single_test: bool,
    /// Planned-test budget; zero is unlimited and retries do not consume it.
    #[arg(long)]
    max_total_tests: Option<u64>,
    /// Explicitly include or exclude simultaneous TCP transmit/receive.
    #[arg(long,action=clap::ArgAction::Set)]
    bidirectional: Option<bool>,
    #[arg(long, default_value = "logs")]
    out: PathBuf,
}
#[derive(Subcommand)]
enum ProfileCommand {
    List,
    Show {
        name: String,
    },
    Save {
        name: String,
        #[arg(long, required_unless_present = "file", conflicts_with = "file")]
        parameters: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Delete {
        name: String,
    },
}
#[derive(Subcommand)]
enum RunCommand {
    List {
        #[arg(default_value = "logs")]
        directory: PathBuf,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Show {
        path: PathBuf,
        #[arg(long)]
        offset: Option<usize>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Compare {
        baseline: PathBuf,
        current: PathBuf,
    },
    Export {
        path: PathBuf,
        destination: PathBuf,
    },
}
#[derive(Subcommand)]
enum HelperCommand {
    Status,
    Register,
    Remove,
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args().collect::<Vec<_>>();
    let throughput = arguments
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .is_some_and(|a| a == "throughput");
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            emit(&json!({"error":Error::validation(error.to_string())}), true);
            return ExitCode::from(if throughput { 11 } else { 1 });
        }
    };
    let compact = cli.json;
    let (value, code) = match dispatch(cli.command).await {
        Ok(result) => result,
        Err(error) => {
            let code = if error.category == ErrorCategory::Cancelled {
                130
            } else if throughput {
                error.category.throughput_exit_code()
            } else {
                1
            };
            (json!({"error":error}), code)
        }
    };
    emit(&value, compact);
    ExitCode::from(code)
}
fn emit(value: &Value, compact: bool) {
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    let result = if compact {
        serde_json::to_writer(&mut output, value)
    } else {
        serde_json::to_writer_pretty(&mut output, value)
    };
    if result.is_ok() {
        let _ = writeln!(output);
    }
}
async fn dispatch(command: Command) -> Result<(Value, u8)> {
    let value = match command {
        Command::Doctor => lantern_runtime::doctor().await,
        Command::Path { command } => {
            return match command {
                PathCommand::Basic(options) => measurement("path_basic", options, None).await,
                PathCommand::Trace(options) => measurement("path_trace", options, None).await,
            };
        }
        Command::Throughput(options) => return measurement("throughput", options, None).await,
        Command::Workflow { name, options } => {
            return measurement(
                "workflow",
                options,
                name.map(|name| match name {
                    WorkflowName::Triage => "triage",
                    WorkflowName::Path => "path",
                    WorkflowName::Throughput => "throughput",
                    WorkflowName::Baseline => "baseline",
                    WorkflowName::WindowsTuning => "windows_tuning",
                }),
            )
            .await;
        }
        Command::Tuning(options) => return measurement("tuning", options, None).await,
        Command::Helper { command } => {
            lantern_runtime::helper_operation(match command {
                HelperCommand::Status => "status",
                HelperCommand::Register => "register",
                HelperCommand::Remove => "remove",
            })
            .await?
        }
        Command::Profiles { store, command } => {
            let store = ProfileStore { path: store };
            match command {
                ProfileCommand::List => json!({"profiles":store.list()?}),
                ProfileCommand::Show { name } => store.get(&name)?,
                ProfileCommand::Save {
                    name,
                    parameters,
                    file,
                } => {
                    let parameters = if let Some(file) = file {
                        read_json(&file, PROFILE_LIMIT)?
                    } else {
                        parse_settings(parameters.as_deref().unwrap_or("{}"))?
                    };
                    store.save(&name, &parameters)?;
                    json!({"saved":name,"path":store.path})
                }
                ProfileCommand::Delete { name } => {
                    json!({"name":name,"deleted":store.delete(&name)?})
                }
            }
        }
        Command::Runs { command } => match command {
            RunCommand::List {
                directory,
                offset,
                limit,
            } => lantern_runtime::list_runs(&directory, offset, limit)?,
            RunCommand::Show {
                path,
                offset,
                limit,
            } => match (offset, limit) {
                (None, None) => json!(reports::read(&path)?),
                (offset, limit) => {
                    reports::read_page(&path, offset.unwrap_or(0), limit.unwrap_or(50))?
                }
            },
            RunCommand::Compare { baseline, current } => {
                reports::compare(&reports::read(&baseline)?, &reports::read(&current)?)
            }
            RunCommand::Export { path, destination } => {
                reports::export_file(&path, &destination)?;
                json!({"exported":destination})
            }
        },
    };
    Ok((value, 0))
}
fn parse_settings(text: &str) -> Result<Value> {
    if text.len() > PROFILE_LIMIT {
        return Err(Error::validation("Settings exceed 1 MiB"));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| Error::validation(e.to_string()))?;
    if !value.is_object() {
        return Err(Error::validation("Settings must be a JSON object"));
    }
    Ok(value)
}
async fn measurement(
    kind: &str,
    options: Measurement,
    workflow: Option<&str>,
) -> Result<(Value, u8)> {
    let mut layers = Vec::new();
    if let Some(name) = options.profile {
        layers.push(
            ProfileStore {
                path: options.profiles_file,
            }
            .get(&name)?,
        );
    }
    if let Some(path) = options.config {
        layers.push(read_json(&path, PROFILE_LIMIT)?);
    }
    let mut explicit = parse_settings(&options.settings)?;
    let map = explicit.as_object_mut().unwrap();
    for (key, value) in [
        ("target", options.target.map(Value::from)),
        ("port", options.port.map(Value::from)),
        ("duration_secs", options.duration.map(Value::from)),
        ("omit_secs", options.omit.map(Value::from)),
        ("protocol", options.protocol.map(Value::from)),
        ("max_total_tests", options.max_total_tests.map(Value::from)),
        ("bidirectional", options.bidirectional.map(Value::from)),
    ] {
        if let Some(value) = value {
            map.insert(key.into(), value);
        }
    }
    if options.single_test {
        map.insert("single_test".into(), json!(true));
    }
    layers.push(explicit);
    let request =
        json!({"capability":kind,"layers":layers,"strict":options.strict,"workflow":workflow});
    let request = lantern_runtime::prepare_local_request(&request)?;
    let plan = lantern_runtime::plan_request(&request)?;
    if options.dry_run {
        return Ok((plan, 0));
    }
    let manager = lantern_runtime::manager::RunManager::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let signal_cancel = cancel.clone();
    let code = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(130));
    let signal_code = code.clone();
    #[cfg(unix)]
    let signal = tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt()).expect("SIGINT handler");
        let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {_=interrupt.recv()=>(),_=terminate.recv()=>signal_code.store(143,std::sync::atomic::Ordering::SeqCst)};
        signal_cancel.cancel();
    });
    #[cfg(not(unix))]
    let signal = tokio::spawn(async move {
        let _ = signal_code;
        let _ = tokio::signal::ctrl_c().await;
        signal_cancel.cancel();
    });
    let result = lantern_runtime::execute_request_with_exit_code(
        &request,
        &options.out,
        &manager,
        &cancel,
        &code,
    )
    .await;
    signal.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_show_defaults_to_full_read_and_accepts_page_options() {
        let full = Cli::try_parse_from(["network-lantern", "runs", "show", "report.csv"]).unwrap();
        assert!(matches!(
            full.command,
            Command::Runs {
                command: RunCommand::Show {
                    offset: None,
                    limit: None,
                    ..
                }
            }
        ));

        let page = Cli::try_parse_from([
            "network-lantern",
            "runs",
            "show",
            "report.csv",
            "--offset",
            "40",
            "--limit",
            "20",
        ])
        .unwrap();
        assert!(matches!(
            page.command,
            Command::Runs {
                command: RunCommand::Show {
                    offset: Some(40),
                    limit: Some(20),
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn runs_show_page_options_use_the_bounded_reader() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = dir.path().join("report.csv");
        std::fs::write(&path, "id\n1\n2\n3\n").unwrap();
        let (value, code) = dispatch(Command::Runs {
            command: RunCommand::Show {
                path,
                offset: Some(1),
                limit: Some(1),
            },
        })
        .await
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(value["rows"][0]["id"], "2");
        assert_eq!(value["total"], 3);
        assert_eq!(value["has_more"], true);
    }
}
