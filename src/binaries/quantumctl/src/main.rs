use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Parser)]
#[command(name = "quantumctl")]
#[command(about = "Control the quantum daemon", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output raw JSON response
    #[arg(global = true, long)]
    json: bool,

    /// Socket path for daemon communication
    #[arg(global = true, long)]
    socket: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Toggle a view (show if hidden, hide if shown)
    Toggle {
        /// View name
        view: String,
    },
    /// Show a view
    Show {
        /// View name
        view: String,
    },
    /// Hide a view
    Hide {
        /// View name
        view: String,
    },
    /// Search for items
    Search {
        /// Query text
        query: String,
    },
    /// List registered providers
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Get system status
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },
    /// Reload theme
    Theme {
        #[command(subcommand)]
        command: ThemeCommand,
    },
    /// Manage timers and alarms
    Timer {
        #[command(subcommand)]
        command: TimerCommand,
    },
    /// Raw JSON-RPC call
    Call {
        /// Method name
        method: String,
        /// JSON parameters
        params: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// List all providers
    List,
}

#[derive(Subcommand)]
enum SystemCommand {
    /// Get system status
    Status,
}

#[derive(Subcommand)]
enum TimerCommand {
    /// Create a timer or alarm
    Create {
        /// Label for the timer
        label: String,
        /// Duration from now (for example 90s, 45m, 1h30m)
        #[arg(long = "in")]
        in_duration: Option<String>,
        /// Time of day as HH:MM
        #[arg(long)]
        at: Option<String>,
        /// Repeat specification (daily, or a weekday list like tue,thu)
        #[arg(long)]
        repeat: Option<String>,
    },
    /// List all timers
    List,
    /// Cancel a timer by id
    Cancel {
        /// Timer id
        id: String,
    },
    /// Dismiss a timer by id
    Dismiss {
        /// Timer id
        id: String,
    },
    /// Edit an existing timer
    Edit {
        /// Timer id
        id: String,
        /// New time of day as HH:MM
        #[arg(long)]
        at: Option<String>,
        /// New repeat specification (daily, or a weekday list like tue,thu)
        #[arg(long)]
        repeat: Option<String>,
        /// New label
        #[arg(long)]
        label: Option<String>,
    },
}

#[derive(Subcommand)]
enum ThemeCommand {
    /// Reload the theme
    Reload,
    /// Switch to a different theme
    Switch {
        /// Theme name
        theme: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let socket_path = cli.socket.unwrap_or_else(|| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("{}/.run", std::env::var("HOME").unwrap_or_default()));
        PathBuf::from(runtime_dir).join("quantum.sock")
    });

    match cli.command {
        Commands::Toggle { view } => {
            let result = call_daemon(&socket_path, "view.toggle", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Show { view } => {
            let result = call_daemon(&socket_path, "view.show", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Hide { view } => {
            let result = call_daemon(&socket_path, "view.hide", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Search { query } => {
            let result = call_daemon(&socket_path, "search", json!({ "text": query })).await?;
            print_response(&result, cli.json);
        }
        Commands::Provider { command } => match command {
            ProviderCommand::List => {
                let result = call_daemon(&socket_path, "provider.list", json!({})).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::System { command } => match command {
            SystemCommand::Status => {
                let result = call_daemon(&socket_path, "system.status", json!({})).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::Theme { command } => match command {
            ThemeCommand::Reload => {
                let result = call_daemon(&socket_path, "theme.reload", json!({})).await?;
                print_response(&result, cli.json);
            }
            ThemeCommand::Switch { theme } => {
                let result =
                    call_daemon(&socket_path, "theme.set", json!({ "theme": theme })).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::Timer { command } => match command {
            TimerCommand::Create {
                label,
                in_duration,
                at,
                repeat,
            } => {
                let start = match (in_duration, at) {
                    (Some(_), Some(_)) => {
                        return Err("provide exactly one of --in or --at, not both".into());
                    }
                    (None, None) => {
                        return Err("provide exactly one of --in or --at".into());
                    }
                    (Some(duration), None) => {
                        if repeat.is_some() {
                            return Err("--repeat cannot be used with --in".into());
                        }
                        let secs = parse_duration(&duration)?;
                        json!({ "kind": "duration", "secs": secs })
                    }
                    (None, Some(time)) => {
                        let (hour, minute) = parse_time_of_day(&time)?;
                        match repeat {
                            Some(repeat_spec) => {
                                let days = parse_repeat(&repeat_spec)?;
                                json!({
                                    "kind": "recurring",
                                    "days": days,
                                    "time": { "hour": hour, "minute": minute }
                                })
                            }
                            None => json!({
                                "kind": "at",
                                "time": { "hour": hour, "minute": minute }
                            }),
                        }
                    }
                };
                let result = call_daemon(
                    &socket_path,
                    "timer.create",
                    json!({ "label": label, "start": start }),
                )
                .await?;
                print_response(&result, cli.json);
            }
            TimerCommand::List => {
                let result = call_daemon(&socket_path, "timer.list", json!({})).await?;
                print_response(&result, cli.json);
            }
            TimerCommand::Cancel { id } => {
                let result =
                    call_daemon(&socket_path, "timer.cancel", json!({ "id": id })).await?;
                print_response(&result, cli.json);
            }
            TimerCommand::Dismiss { id } => {
                let result =
                    call_daemon(&socket_path, "timer.dismiss", json!({ "id": id })).await?;
                print_response(&result, cli.json);
            }
            TimerCommand::Edit {
                id,
                at,
                repeat,
                label,
            } => {
                let mut changes = serde_json::Map::new();
                if let Some(label) = label {
                    changes.insert("label".to_string(), json!(label));
                }
                if let Some(time) = at {
                    let (hour, minute) = parse_time_of_day(&time)?;
                    changes.insert("time".to_string(), json!({ "hour": hour, "minute": minute }));
                }
                if let Some(repeat_spec) = repeat {
                    let days = parse_repeat(&repeat_spec)?;
                    changes.insert("days".to_string(), json!(days));
                }
                let result = call_daemon(
                    &socket_path,
                    "timer.edit",
                    json!({ "id": id, "changes": Value::Object(changes) }),
                )
                .await?;
                print_response(&result, cli.json);
            }
        },
        Commands::Call { method, params } => {
            let params_value = if let Some(p) = params {
                serde_json::from_str(&p)?
            } else {
                json!({})
            };
            let result = call_daemon(&socket_path, &method, params_value).await?;
            print_response(&result, cli.json);
        }
    }

    Ok(())
}

async fn call_daemon(
    socket_path: &std::path::Path,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Connect to daemon
    let stream = UnixStream::connect(socket_path).await.map_err(|e| {
        format!(
            "Failed to connect to daemon at {}: {}",
            socket_path.display(),
            e
        )
    })?;

    // Build JSON-RPC request
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });

    // Split for independent read/write halves so we can buffer the reader.
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send request as a single newline-delimited JSON-RPC line.
    let request_str = request.to_string() + "\n";
    write_half.write_all(request_str.as_bytes()).await?;
    write_half.flush().await?;

    // Read exactly one newline-delimited response line. The daemon's IPC
    // server (see src/infrastructure/ipc/src/server.rs) writes each
    // response as JSON followed by `\n`, so `read_line` consumes the
    // entire response regardless of its size. The previous fixed 4 KB
    // read truncated large responses such as search results containing
    // many desktop entries plus Hyprland windows.
    let mut response_str = String::new();
    reader.read_line(&mut response_str).await?;

    // Parse response
    let response: Value = serde_json::from_str(&response_str)?;

    // Check for JSON-RPC error
    if let Some(error) = response.get("error") {
        if !error.is_null() {
            eprintln!(
                "RPC error: {}",
                error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            );
            std::process::exit(1);
        }
    }

    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn print_response(value: &Value, json_mode: bool) {
    if json_mode {
        println!("{}", value);
    } else {
        // Human-readable output
        if value.is_null() {
            println!("(no response)");
        } else if value.is_object() {
            print_object(value, 0);
        } else if value.is_array() {
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        } else {
            println!("{}", value);
        }
    }
}

fn print_object(value: &Value, indent: usize) {
    if let Some(obj) = value.as_object() {
        for (key, val) in obj {
            let prefix = " ".repeat(indent);
            if val.is_object() {
                println!("{}{}:", prefix, key);
                print_object(val, indent + 2);
            } else if val.is_array() {
                println!("{}{}:", prefix, key);
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        if item.is_object() {
                            print_object(item, indent + 2);
                        } else {
                            println!("{}{}", " ".repeat(indent + 2), item);
                        }
                    }
                }
            } else {
                println!("{}{}: {}", prefix, key, val);
            }
        }
    }
}

/// Parse a duration made of concatenated `<num>h`, `<num>m`, `<num>s`
/// components (for example `90s`, `45m`, `1h30m`, `2h`) into a total number
/// of seconds.
///
/// A bare integer with no unit (for example `90`) is rejected to avoid
/// ambiguity between seconds and minutes. Empty input, unknown units, and
/// any other garbage are also rejected.
fn parse_duration(input: &str) -> Result<u64, String> {
    if input.is_empty() {
        return Err("empty duration".to_string());
    }

    let mut total: u64 = 0;
    let mut current: Option<u64> = None;
    let mut saw_component = false;

    for ch in input.chars() {
        if let Some(digit) = ch.to_digit(10) {
            let value = current.unwrap_or(0);
            current = Some(
                value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(u64::from(digit)))
                    .ok_or_else(|| format!("duration number too large in '{}'", input))?,
            );
            continue;
        }

        let multiplier = match ch {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            other => return Err(format!("unknown duration unit '{}' in '{}'", other, input)),
        };

        let number =
            current.ok_or_else(|| format!("missing number before '{}' in '{}'", ch, input))?;
        total = total
            .checked_add(
                number
                    .checked_mul(multiplier)
                    .ok_or_else(|| format!("duration too large in '{}'", input))?,
            )
            .ok_or_else(|| format!("duration too large in '{}'", input))?;
        current = None;
        saw_component = true;
    }

    if current.is_some() {
        return Err(format!("trailing number without a unit in '{}'", input));
    }
    if !saw_component {
        return Err(format!("no duration components in '{}'", input));
    }

    Ok(total)
}

/// Parse a `HH:MM` time of day, validating `hour <= 23` and `minute <= 59`.
fn parse_time_of_day(input: &str) -> Result<(u8, u8), String> {
    let (hour_str, minute_str) = input
        .split_once(':')
        .ok_or_else(|| format!("expected HH:MM, got '{}'", input))?;

    let hour: u8 = hour_str
        .parse()
        .map_err(|_| format!("invalid hour in '{}'", input))?;
    let minute: u8 = minute_str
        .parse()
        .map_err(|_| format!("invalid minute in '{}'", input))?;

    if hour > 23 {
        return Err(format!("hour out of range in '{}'", input));
    }
    if minute > 59 {
        return Err(format!("minute out of range in '{}'", input));
    }

    Ok((hour, minute))
}

/// Parse a repeat specification into a list of canonical lowercase weekday
/// names.
///
/// `"daily"` expands to all seven days. A comma list such as `"tue,thu"` or
/// `"tuesday,thursday"` expands to the matching full lowercase names; both
/// three-letter abbreviations and full names are accepted, case-insensitive.
/// Bare `"weekly"` is rejected because weekly repeats require explicit days.
/// Unknown tokens are rejected.
fn parse_repeat(input: &str) -> Result<Vec<String>, String> {
    let canonical = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];

    let lowered = input.trim().to_lowercase();
    if lowered == "daily" {
        return Ok(canonical.iter().map(|day| day.to_string()).collect());
    }
    if lowered == "weekly" {
        return Err("'weekly' requires explicit days (for example tue,thu)".to_string());
    }

    let mut days = Vec::new();
    for token in lowered.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(format!("empty weekday token in '{}'", input));
        }
        let matched = canonical
            .iter()
            .find(|day| **day == token || day.starts_with(token) && token.len() == 3)
            .ok_or_else(|| format!("unknown weekday '{}'", token))?;
        days.push(matched.to_string());
    }

    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_components() {
        assert_eq!(parse_duration("90s").unwrap(), 90);
        assert_eq!(parse_duration("45m").unwrap(), 2700);
        assert_eq!(parse_duration("1h30m").unwrap(), 5400);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert!(parse_duration("banana").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn parse_duration_bare_integer_is_error() {
        // A bare integer with no unit is rejected to avoid ambiguity.
        assert!(parse_duration("90").is_err());
    }

    #[test]
    fn parse_time_of_day_hhmm() {
        assert_eq!(parse_time_of_day("17:15").unwrap(), (17, 15));
        assert_eq!(parse_time_of_day("00:00").unwrap(), (0, 0));
        assert!(parse_time_of_day("25:00").is_err());
        assert!(parse_time_of_day("10:60").is_err());
    }

    #[test]
    fn parse_repeat_lists() {
        assert_eq!(parse_repeat("daily").unwrap().len(), 7);
        assert_eq!(parse_repeat("tue,thu").unwrap().len(), 2);
        assert_eq!(
            parse_repeat("tuesday,thursday").unwrap(),
            vec!["tuesday".to_string(), "thursday".to_string()]
        );
        assert!(parse_repeat("weekly").is_err());
        assert!(parse_repeat("funday").is_err());
    }
}
