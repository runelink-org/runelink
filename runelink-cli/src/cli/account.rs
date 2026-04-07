use runelink_client::requests;
use runelink_types::SignupRequest;
use uuid::Uuid;

use crate::{
    cli::input::{read_input, read_input_preserving_whitespace},
    error::CliError,
    storage::{AccountConfig, resolve_api_url},
    storage_auth::AccountAuth,
    util::{
        self, parse_host_input, parse_user_ref_input, parse_username_input,
    },
};

use super::{
    config::{DefaultAccountArgs, handle_default_account_commands},
    context::CliContext,
    select::select_inline,
};

#[derive(clap::Args, Debug)]
pub struct AccountArgs {
    #[clap(subcommand)]
    pub command: AccountCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum AccountCommands {
    /// List accounts
    List,
    /// Create a new account
    Create(NameAndHostArgs),
    /// Login to an account (store authentication tokens)
    Login(LoginArgs),
    /// Logout from an account (remove authentication tokens)
    Logout(LogoutArgs),
    /// Show authentication status for an account
    Status(StatusArgs),
    /// Delete an account (deletes the underlying user)
    Delete(DeleteAccountArgs),
    /// Manage default account
    Default(DefaultAccountArgs),
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

#[derive(clap::Args, Debug)]
pub struct LoginArgs {
    /// The account's username
    #[clap(long)]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct LogoutArgs {
    /// The account's username
    #[clap(long)]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct StatusArgs {
    /// The account's username
    #[clap(long)]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct DeleteAccountArgs {
    #[clap(long)]
    pub name: Option<String>,
    #[clap(long)]
    pub host: Option<String>,
}

pub async fn handle_account_commands(
    ctx: &mut CliContext<'_>,
    account_args: &AccountArgs,
) -> Result<(), CliError> {
    match &account_args.command {
        AccountCommands::List => {
            if ctx.config.accounts.is_empty() {
                println!("No accounts.");
                return Ok(());
            }
            for account in ctx.config.accounts.iter() {
                let prefix = util::get_prefix(
                    &account.user_ref,
                    ctx.config.default_account.as_ref(),
                    ctx.config.accounts.len(),
                );
                println!("{prefix}{account}");
            }
        }

        AccountCommands::Create(create_args) => {
            let raw_host = match &create_args.host {
                Some(host) => host.clone(),
                None => read_input_preserving_whitespace("Host: ")?
                    .ok_or_else(|| {
                        CliError::InvalidArgument("Host is required.".into())
                    })?,
            };
            let raw_name = match &create_args.name {
                Some(name) => name.clone(),
                None => read_input_preserving_whitespace("Name: ")?
                    .ok_or_else(|| {
                        CliError::InvalidArgument("Name is required.".into())
                    })?,
            };
            let host = parse_host_input(&raw_host, ctx.strict_input)?;
            let name = parse_username_input(&raw_name, ctx.strict_input)?;
            let password = read_input("Password: ")?.ok_or_else(|| {
                CliError::InvalidArgument("Password is required.".into())
            })?;
            let api_url =
                resolve_api_url(ctx.client, ctx.config, &host).await?;
            let signup_req = SignupRequest { name, password };
            let user =
                requests::auth::signup(ctx.client, &api_url, &signup_req)
                    .await?;
            ctx.config.get_or_create_account_config(user.as_ref());
            ctx.config.save()?;
            println!("Created account: {user}");
        }

        AccountCommands::Login(login_args) => {
            // Get or discover account
            let account = if let (Some(name), Some(host)) =
                (&login_args.name, &login_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                // Try to find existing account in config
                if let Some(acc) =
                    ctx.config.get_account_config(user_ref.clone())
                {
                    acc
                } else {
                    // Account doesn't exist, fetch it from server
                    let api_url =
                        resolve_api_url(ctx.client, ctx.config, host).await?;
                    let user = requests::users::fetch_by_ref(
                        ctx.client,
                        &api_url,
                        user_ref.clone(),
                    )
                    .await?;
                    ctx.config.get_or_create_account_config(user.as_ref());
                    ctx.config.save()?;
                    ctx.config
                        .get_account_config(user_ref)
                        .expect("Account should exist after creation")
                }
            } else {
                ctx.account.ok_or(CliError::MissingAccount)?
            };

            let account_user_ref = account.user_ref.clone();
            let account_display = account.to_string();

            let password = read_input("Password: ")?.ok_or_else(|| {
                CliError::InvalidArgument("Password is required.".into())
            })?;

            // TODO: Don't generate a random client_id for each session
            let client_id = Uuid::new_v4().to_string();

            let api_url =
                resolve_api_url(ctx.client, ctx.config, &account_user_ref.host)
                    .await?;
            let token_response = requests::auth::token_password(
                ctx.client,
                &api_url,
                &account_user_ref.name,
                &password,
                None,
                Some(&client_id),
            )
            .await?;

            // Store auth data
            let account_auth = AccountAuth {
                refresh_token: token_response.refresh_token,
                access_token: Some(token_response.access_token),
                expires_at: Some(
                    time::OffsetDateTime::now_utc().unix_timestamp()
                        + token_response.expires_in,
                ),
                client_id: Some(client_id),
                scope: None,
            };
            ctx.auth_cache.set(&account_user_ref, account_auth);
            ctx.auth_cache.save()?;

            println!("Logged in successfully: {account_display}");
        }

        AccountCommands::Logout(logout_args) => {
            let account = if let (Some(name), Some(host)) =
                (&logout_args.name, &logout_args.host)
            {
                ctx.config
                    .get_account_config(parse_user_ref_input(
                        name,
                        host,
                        ctx.strict_input,
                    )?)
                    .ok_or_else(|| {
                        CliError::InvalidArgument("Account not found.".into())
                    })?
            } else {
                ctx.account.ok_or(CliError::MissingAccount)?
            };

            if ctx.auth_cache.remove(&account.user_ref).is_some() {
                ctx.auth_cache.save()?;
                println!("Logged out successfully: {account}");
            } else {
                println!("No authentication data found for: {account}");
            }
        }

        AccountCommands::Status(status_args) => {
            let account = if let (Some(name), Some(host)) =
                (&status_args.name, &status_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config.get_account_config(user_ref).ok_or_else(|| {
                    CliError::InvalidArgument("Account not found.".into())
                })?
            } else {
                ctx.account.ok_or(CliError::MissingAccount)?
            };

            if let Some(auth) = ctx.auth_cache.get(&account.user_ref) {
                println!("Account: {account}");
                println!("  Authenticated: Yes");
                if let Some(expires_at) = auth.expires_at {
                    let expires =
                        time::OffsetDateTime::from_unix_timestamp(expires_at)
                            .unwrap_or_else(|_| {
                                time::OffsetDateTime::now_utc()
                            });
                    let now = time::OffsetDateTime::now_utc();
                    if expires > now {
                        let remaining = expires - now;
                        println!(
                            "  Access token expires in: {} seconds",
                            remaining.whole_seconds()
                        );
                    } else {
                        println!("  Access token: Expired");
                    }
                } else {
                    println!("  Access token: Not cached");
                }
                if let Some(ref client_id) = auth.client_id {
                    println!("  Client ID: {}", client_id);
                }
                if let Some(ref scope) = auth.scope {
                    println!("  Scope: {}", scope);
                }
            } else {
                println!("Account: {account}");
                println!("  Authenticated: No");
            }
        }

        AccountCommands::Delete(delete_args) => {
            if ctx.config.accounts.is_empty() {
                return Err(CliError::InvalidArgument(
                    "No accounts in local config.".into(),
                ));
            }

            let user_ref = if let (Some(name), Some(host)) =
                (&delete_args.name, &delete_args.host)
            {
                let account = ctx
                    .config
                    .get_account_config(parse_user_ref_input(
                        name,
                        host,
                        ctx.strict_input,
                    )?)
                    .ok_or_else(|| {
                        CliError::InvalidArgument(
                            "Account not found in local config.".into(),
                        )
                    })?;
                account.user_ref.clone()
            } else {
                let account = select_inline(
                    &ctx.config.accounts,
                    "Select account to delete",
                    AccountConfig::to_string,
                )?
                .ok_or(CliError::Cancellation)?;
                println!();
                account.user_ref.clone()
            };

            let api_url =
                resolve_api_url(ctx.client, ctx.config, &user_ref.host).await?;
            let access_token =
                ctx.get_access_token_for(&user_ref, &api_url).await?;

            requests::users::delete(
                ctx.client,
                &api_url,
                &access_token,
                user_ref.clone(),
            )
            .await?;

            ctx.auth_cache.remove(&user_ref);
            ctx.config.accounts.retain(|a| a.user_ref != user_ref);
            if ctx.config.default_account.as_ref() == Some(&user_ref) {
                ctx.config.default_account = None;
            }
            ctx.config.save()?;
            ctx.auth_cache.save()?;

            println!("Deleted account/user: {}", user_ref);
        }

        AccountCommands::Default(default_args) => {
            handle_default_account_commands(ctx, default_args).await?;
        }
    }
    Ok(())
}
