//! `vpush`: the server and its admin command line in one file.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use vpush_server::admin::{self, Request};
use vpush_server::config::{self, Config};
use vpush_server::logging::parse_duration;
use vpush_server::store::SqliteStore;
use vpush_server::version;

#[derive(Parser)]
#[command(name = "vpush", about = "Veydan push server", version = version_line().leak() as &str)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn version_line() -> String {
    format!(
        "{} ({}, built at {})",
        version::VERSION,
        version::GIT_SHA,
        version::built_at()
    )
}

#[derive(Args, Clone)]
struct ConfigArg {
    /// Config file.
    #[arg(long, env = "VPUSH_CONFIG", default_value = config::DEFAULT_PATH)]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server.
    Serve(ConfigArg),
    /// Read the config and report every problem in it.
    CheckConfig(ConfigArg),
    /// Steer the running server.
    Ctl {
        #[command(flatten)]
        config: ConfigArg,
        /// Admin socket; by default the one named in the config.
        #[arg(long, env = "VPUSH_SOCKET")]
        socket: Option<PathBuf>,
        #[command(subcommand)]
        command: Ctl,
    },
    /// Write a whole copy of the database to a file, as it is, schema and all.
    Backup {
        #[command(flatten)]
        config: ConfigArg,
        /// The file to write; it must not exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Print the release name, as used for the release directory.
    ReleaseName,
}

#[derive(Subcommand)]
enum Ctl {
    /// Is the server alive, and which build is it.
    Health,
    /// Log levels.
    #[command(subcommand)]
    Log(LogCmd),
    /// The devices of one owner, and what is watched for each.
    Devices {
        /// The owner's public key: npub or hex.
        owner: String,
    },
    /// The relays on the line, and how each is doing.
    Relays,
    /// How many devices and owners there are, and what was counted since the start.
    Stats,
    /// Send one push to one token, to see that pushes arrive.
    TestPush {
        /// App id, as in the config.
        #[arg(long)]
        app: String,
        /// fcm, apns, unifiedpush.
        #[arg(long, default_value = "fcm")]
        provider: String,
        /// Token of the device.
        #[arg(long)]
        token: String,
    },
}

#[derive(Subcommand)]
enum LogCmd {
    /// Levels in force, and the overrides with their remaining time.
    Show,
    /// Change levels without a restart: `debug`, `relay=trace`, `relay=debug,api=debug`.
    Set {
        spec: String,
        /// Take the change back after this long: 90s, 15m, 2h. An hour when
        /// not given, a day (1d) at most.
        #[arg(long = "for", value_name = "DURATION", default_value = "1h")]
        ttl: String,
    },
    /// Remove every override; the config levels are in force again.
    Reset,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vpush: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve(arg) => runtime()?.block_on(vpush_server::serve(arg.config)),
        Command::Backup { config, out } => {
            let config = Config::load(&config.config)?;
            runtime()?.block_on(SqliteStore::snapshot(&config.store.path, &out))?;
            println!("database {} saved to {}", config.store.path.display(), out.display());
            Ok(())
        }
        Command::CheckConfig(arg) => {
            let config = Config::load(&arg.config)?;
            println!("config {} is fine", arg.config.display());
            for (key, value) in config.summary() {
                println!("  {key} = {value}");
            }
            Ok(())
        }
        Command::Ctl {
            config,
            socket,
            command,
        } => {
            let socket = match socket {
                Some(s) => s,
                None => Config::load(&config.config)?.admin.socket,
            };
            let request = match command {
                Ctl::Health => Request::Health,
                Ctl::Log(LogCmd::Show) => Request::LogShow,
                Ctl::Log(LogCmd::Reset) => Request::LogReset,
                Ctl::Devices { owner } => Request::Devices { owner },
                Ctl::Stats => Request::Stats,
                Ctl::Relays => Request::Relays,
                Ctl::TestPush {
                    app,
                    provider,
                    token,
                } => Request::TestPush {
                    app,
                    provider: provider.parse().map_err(anyhow::Error::msg)?,
                    token,
                },
                Ctl::Log(LogCmd::Set { spec, ttl }) => Request::LogSet {
                    spec,
                    ttl_secs: parse_duration(&ttl)
                        .map_err(anyhow::Error::msg)?
                        .as_secs(),
                },
            };
            let answer = runtime()?.block_on(admin::call(&socket, &request))?;
            println!("{}", serde_json::to_string_pretty(&answer)?);
            Ok(())
        }
        Command::ReleaseName => {
            println!("{}", version::release_name());
            Ok(())
        }
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}
