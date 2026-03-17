use runelink_client::{requests, util::get_api_url};
use runelink_types::{SignupRequest, UserRef};
use uuid::Uuid;

use crate::{
    cli::input::read_input, error::CliError, storage::AccountConfig,
    storage_auth::AccountAuth, util,
};

use super::{
    config::{DefaultAccountArgs, handle_default_account_commands},
    context::CliContext,
    input::unwrap_or_prompt,
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
            let host =
                unwrap_or_prompt(create_args.host.clone(), "Host")?;
            let name = unwrap_or_prompt(create_args.name.clone(), "Name")?;
            let password = read_input("Password: ")?.ok_or_else(|| {
                CliError::InvalidArgument("Password is required.".into())
            })?;
            let api_url = get_api_url(&host);
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
                let user_ref = UserRef::new(name.clone(), host.clone());
                // Try to find existing account in config
                if let Some(acc) =
                    ctx.config.get_account_config(user_ref.clone())
                {
                    acc
                } else {
                    // Account doesn't exist, fetch it from server
                    let api_url = get_api_url(host);
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

            let password = read_input("Password: ")?.ok_or_else(|| {
                CliError::InvalidArgument("Password is required.".into())
            })?;

            // TODO: Don't generate a random client_id for each session
            let client_id = Uuid::new_v4().to_string();

            let api_url = get_api_url(&account.user_ref.host);
            let token_response = requests::auth::token_password(
                ctx.client,
                &api_url,
                &account.user_ref.name,
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
            ctx.auth_cache.set(&account.user_ref, account_auth);
            ctx.auth_cache.save()?;

            println!("Logged in successfully: {account}");
        }

        AccountCommands::Logout(logout_args) => {
            let account = if let (Some(name), Some(host)) =
                (&logout_args.name, &logout_args.host)
            {
                ctx.config
                    .get_account_config(UserRef::new(
                        name.clone(),
                        host.clone(),
                    ))
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
                let user_ref = UserRef::new(name.clone(), host.clone());
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
                    .get_account_config(UserRef::new(
                        name.clone(),
                        host.clone(),
                    ))
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

            let api_url = get_api_url(&user_ref.host);
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
