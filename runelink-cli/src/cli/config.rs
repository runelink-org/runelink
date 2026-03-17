use super::{context::CliContext, select::select_inline};
use crate::error::CliError;
use crate::storage::AccountConfig;
use runelink_types::UserRef;

#[derive(clap::Args, Debug)]
pub struct ConfigArgs {
    #[clap(subcommand)]
    pub command: ConfigCommands,
}

#[allow(clippy::enum_variant_names)]
#[derive(clap::Subcommand, Debug)]
pub enum ConfigCommands {
    /// Manage default account
    DefaultAccount(DefaultAccountArgs),
}

pub async fn handle_config_commands(
    ctx: &mut CliContext<'_>,
    config_args: &ConfigArgs,
) -> Result<(), CliError> {
    match &config_args.command {
        ConfigCommands::DefaultAccount(default_account_args) => {
            handle_default_account_commands(ctx, default_account_args).await?;
        }
    }
    Ok(())
}

// DEFAULT HOST

#[derive(clap::Args, Debug)]
pub struct DefaultAccountArgs {
    #[clap(subcommand)]
    pub command: DefaultAccountCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum DefaultAccountCommands {
    /// Show the default account
    Get,
    /// Set the default account
    Set(NameAndHostArgs),
}

#[derive(clap::Args, Debug)]
pub struct NameAndHostArgs {
    /// The account's username
    #[clap(long)]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long)]
    pub host: Option<String>,
}

pub async fn handle_default_account_commands(
    ctx: &mut CliContext<'_>,
    default_args: &DefaultAccountArgs,
) -> Result<(), CliError> {
    match &default_args.command {
        DefaultAccountCommands::Get => {
            if let Some(account) = ctx.config.get_default_account() {
                println!("{account}");
            } else {
                println!("No default host set.");
            }
        }

        DefaultAccountCommands::Set(set_args) => {
            let account = if let (Some(name), Some(host)) =
                (&set_args.name, &set_args.host)
            {
                let user_ref = UserRef::new(name.clone(), host.clone());
                ctx.config.get_account_config(user_ref).ok_or_else(|| {
                    CliError::InvalidArgument("Account not found.".into())
                })?
            } else {
                let tmp = select_inline(
                    &ctx.config.accounts,
                    "Select account",
                    AccountConfig::to_string,
                )?
                .ok_or(CliError::Cancellation)?;
                println!();
                tmp
            }
            .clone();
            ctx.config.default_account = Some(account.user_ref.clone());
            ctx.config.save()?;
            println!("Set default account: {account}");
        }
    }
    Ok(())
}
