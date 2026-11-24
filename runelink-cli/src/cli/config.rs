use super::{context::CliContext, select::select_inline};
use crate::error::CliError;
use crate::storage::AccountConfig;
use crate::util::parse_user_ref_input;

#[derive(clap::Args, Debug)]
pub struct ConfigArgs {
    #[clap(subcommand)]
    pub command: ConfigCommands,
}

#[allow(clippy::enum_variant_names)]
#[derive(clap::Subcommand, Debug)]
pub enum ConfigCommands {
    /// Manage the active account
    ActiveAccount(ActiveAccountArgs),
}

pub async fn handle_config_commands(
    ctx: &mut CliContext<'_>,
    config_args: &ConfigArgs,
) -> Result<(), CliError> {
    match &config_args.command {
        ConfigCommands::ActiveAccount(active_account_args) => {
            handle_active_account_commands(ctx, active_account_args).await?;
        }
    }
    Ok(())
}

// ACTIVE ACCOUNT

#[derive(clap::Args, Debug)]
pub struct ActiveAccountArgs {
    #[clap(subcommand)]
    pub command: ActiveAccountCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum ActiveAccountCommands {
    /// Show the active account
    Get,
    /// Set the active account
    Set(NameAndHostArgs),
}

#[derive(clap::Args, Clone, Debug)]
pub struct NameAndHostArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
}

pub async fn handle_active_account_commands(
    ctx: &mut CliContext<'_>,
    active_args: &ActiveAccountArgs,
) -> Result<(), CliError> {
    match &active_args.command {
        ActiveAccountCommands::Get => {
            let account = ctx.config.get_active_account();
            let user_ref = account.map(|account| &account.user_ref);
            ctx.output(&user_ref, || match account {
                Some(account) => println!("{account}"),
                None => println!("No active account set."),
            })?;
        }

        ActiveAccountCommands::Set(set_args) => {
            let account = if let (Some(name), Some(host)) =
                (&set_args.name, &set_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config
                    .get_account_config(user_ref.clone())
                    .ok_or_else(|| CliError::account_not_found(&user_ref))?
            } else {
                if ctx.config.accounts.is_empty() {
                    return Err(CliError::InvalidArgument(
                        "No accounts in local config.".into(),
                    ));
                }
                let tmp = select_inline(
                    &ctx.config.accounts,
                    "Select account",
                    "--name and --host",
                    AccountConfig::to_string,
                )?
                .ok_or(CliError::Cancellation)?;
                eprintln!();
                tmp
            }
            .clone();
            ctx.config.active_account = Some(account.user_ref.clone());
            ctx.config.save()?;
            ctx.output(&account.user_ref, || {
                println!("Set active account: {account}")
            })?;
        }
    }
    Ok(())
}
