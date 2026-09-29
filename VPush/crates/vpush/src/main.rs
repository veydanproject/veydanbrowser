//! `vpush`: the server and its admin command line in one file.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use vpush_server::admin::{self, Request};
use vpush_server::config::{self, Config};
use vpush_server::logging::parse_duration;
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
        #[arg(long, default_value = "VPush")]
        title: String,
        #[arg(long, default_value = "Test push")]
        body: String,
        /// No title and no text: the device handles it without showing anything.
        #[arg(long)]
        silent: bool,
    },
}

#[derive(Subcommand)]
enum LogCmd {
    /// Levels in force, and the overrides with their remaining time.
    Show,
    /// Change levels without a restart: `debug`, `relay=trace`, `relay=debug,api=debug`.
    Set {
        spec: String,
        /// Take the change back after this long: 90s, 15m, 2h, 1d.
        #[arg(long = "for", value_name = "DURATION")]
        ttl: Option<String>,
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
                Ctl::TestPush {
                    app,
                    provider,
                    token,
                    title,
                    body,
                    silent,
                } => Request::TestPush {
                    app,
                    provider: provider.parse().map_err(anyhow::Error::msg)?,
                    token,
                    title: (!silent).then_some(title),
                    body: (!silent).then_some(body),
                },
                Ctl::Log(LogCmd::Set { spec, ttl }) => Request::LogSet {
                    spec,
                    ttl_secs: ttl
                        .map(|t| parse_duration(&t).map(|d| d.as_secs()))
                        .transpose()
                        .map_err(anyhow::Error::msg)?,
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
