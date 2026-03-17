use clap::CommandFactory;
use clap_complete::Shell;
use context::CliContext;
use log::LevelFilter;
use reqwest::Client;
use runelink_types::UserRef;

use crate::{error::CliError, storage::AppConfig, storage_auth::AuthCache};

pub mod account;
pub mod channels;
pub mod config;
pub mod context;
pub mod input;
pub mod messages;
pub mod select;
pub mod servers;
pub mod users;

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[clap(name = "rune")]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Commands,
    /// Optional: The account's username
    #[clap(long)]
    pub name: Option<String>,
    /// Optional: The host name of the account's host
    #[clap(long)]
    pub host: Option<String>,
    /// Increase logging verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Manage accounts
    Account(account::AccountArgs),
    /// Manage channels
    Channel(channels::ChannelArgs),
    /// Manage messages
    Message(messages::MessageArgs),
    /// Manage servers
    Server(servers::ServerArgs),
    /// Manage users
    User(users::UserArgs),
    /// Manage config
    Config(config::ConfigArgs),
    /// Generate shell completion scripts
    Completions(CompletionsArgs),
}

#[derive(clap::Args, Debug)]
pub struct CompletionsArgs {
    #[clap(value_parser = clap::value_parser!(Shell))]
    pub shell: Shell,
}

fn init_logging(verbosity: u8) {
    let level = match verbosity {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    env_logger::Builder::new().filter_level(level).init();
}

pub async fn handle_cli(
    client: &Client,
    cli: &Cli,
    config: &mut AppConfig,
    auth_cache: &mut AuthCache,
) -> Result<(), CliError> {
    init_logging(cli.verbose);
    let account_owned = match (&cli.name, &cli.host) {
        (Some(name), Some(host)) => {
            let user_ref = UserRef::new(name.clone(), host.clone());
            config.get_account_config(user_ref).cloned()
        }
        _ => config.get_default_account().cloned(),
    };
    let mut ctx_owned = CliContext {
        client,
        config,
        auth_cache,
        account: account_owned.as_ref(),
    };
    let ctx = &mut ctx_owned;

    match &cli.command {
        Commands::Account(args) => {
            account::handle_account_commands(ctx, args).await?;
        }
        Commands::Channel(args) => {
            channels::handle_channel_commands(ctx, args).await?;
        }
        Commands::Message(args) => {
            messages::handle_message_commands(ctx, args).await?;
        }
        Commands::Server(args) => {
            servers::handle_server_commands(ctx, args).await?;
        }
        Commands::User(args) => {
            users::handle_user_commands(ctx, args).await?;
        }
        Commands::Config(args) => {
            config::handle_config_commands(ctx, args).await?;
        }
        Commands::Completions(args) => {
            let mut cmd = Cli::command();
            let cmd_name = cmd.get_name().to_string();
            clap_complete::generate(
                args.shell,
                &mut cmd,
                cmd_name,
                &mut std::io::stdout(),
            );
        }
    }
    Ok(())
}
