use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod client;
mod config;
mod discover;
mod server;
mod service;
mod store;
#[cfg(windows)]
mod window;
mod ui;
mod util;

#[derive(Parser, Debug)]
#[command(
    name = "phonesync",
    version,
    about = "Auto-sync your phone's photos, videos and files to your laptop over Wi-Fi",
    long_about = None
)]
struct Cli {
    /// Storage root. Defaults to %USERPROFILE%\PhoneSync
    #[arg(long, global = true, env = "PHONESYNC_ROOT")]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the receiver on this laptop (web UI + upload API)
    Serve(ServeArgs),
    /// Desktop app: receiver + native window ("Start" menu entry)
    App(ServeArgs),
    /// Push files or folders to a running receiver
    Send(SendArgs),
    /// Find receivers advertising themselves on the local network
    Discover {
        /// How long to listen, in seconds
        #[arg(long, default_value_t = 3)]
        seconds: u64,
    },
    /// Print the sync URL, token and a QR code
    Info {
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
    /// Install or remove the "start with Windows" scheduled task
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Allow incoming connections through Windows Firewall (needs admin)
    Firewall {
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
}

#[derive(clap::Args, Debug, Clone)]
struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value_t = 8787)]
    port: u16,
    /// Address to bind. 0.0.0.0 = reachable from the phone
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,
    /// Disable the access token (anyone on your Wi-Fi can upload)
    #[arg(long)]
    open: bool,
    /// Do not print the QR code
    #[arg(long)]
    no_qr: bool,
    /// Open the dashboard in the default browser
    #[arg(long)]
    dashboard: bool,
    /// Hide the console window (used by the Windows autostart task)
    #[arg(long, hide = true)]
    hide_console: bool,
    /// How this laptop identifies itself on the network
    #[arg(long)]
    name: Option<String>,
}

impl Default for ServeArgs {
    fn default() -> Self {
        Self {
            port: 8787,
            bind: "0.0.0.0".into(),
            open: false,
            no_qr: false,
            dashboard: false,
            hide_console: false,
            name: None,
        }
    }
}

#[derive(clap::Args, Debug)]
struct SendArgs {
    /// Files and/or folders to send
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Receiver URL (http://ip:port) or a name found via discovery
    #[arg(long, short, default_value = "discover")]
    to: String,
    /// Access token of the receiver
    #[arg(long, env = "PHONESYNC_TOKEN")]
    token: Option<String>,
    /// Keep watching the folders and send new files automatically
    #[arg(long)]
    watch: bool,
    /// Device name shown on the laptop
    #[arg(long)]
    device: Option<String>,
    /// Skip files already sent in a previous run of this watcher
    #[arg(long)]
    state: Option<PathBuf>,
    /// Only send files matching these extensions (comma separated)
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Print what would be sent without uploading
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum ServiceAction {
    Install,
    Uninstall,
    Status,
}

fn launch_app(cfg: config::Config, args: ServeArgs) -> Result<()> {
    #[cfg(windows)]
    {
        window::run(cfg, args)
    }
    #[cfg(not(windows))]
    {
        // No webview shell elsewhere; the browser dashboard is the fallback.
        let mut args = args;
        args.dashboard = true;
        server::run(cfg, args)
    }
}

fn main() -> Result<()> {
    // Double-clicking the exe passes no arguments: that should open the window,
    // not print clap's help and vanish.
    let opened_directly = std::env::args_os().len() == 1;
    let cli = Cli::parse();
    let cfg = config::Config::load(cli.root)?;

    let filter = EnvFilter::try_from_env("PHONESYNC_LOG")
        .unwrap_or_else(|_| EnvFilter::new("phonesync=info,tower_http=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();

    let command = cli.cmd.unwrap_or(Command::App(ServeArgs::default()));
    if opened_directly {
        // Only hides a console this process owns; a terminal someone launched
        // it from stays open.
        util::hide_console_if_private();
    }

    match command {
        Command::Serve(args) => {
            if args.hide_console {
                util::hide_console_window();
            }
            server::run(cfg, args)
        }
        Command::App(args) => launch_app(cfg, args),
        Command::Send(args) => client::run(cfg, args),
        Command::Discover { seconds } => discover::run(seconds),
        Command::Info { port } => server::print_info(&cfg, port),
        Command::Service { action } => service::run(action),
        Command::Firewall { port } => service::firewall(port),
    }
}
