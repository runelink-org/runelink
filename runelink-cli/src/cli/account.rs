use reqwest::StatusCode;
use runelink_client::{
    requests,
    util::{pad_host, strip_default_port},
};
use runelink_types::{SignupRequest, UserRef};
use serde::Serialize;
use time::OffsetDateTime;

use crate::{
    cli::input::{
        is_non_interactive, missing_argument, read_input_preserving_whitespace,
        read_password, read_password_stdin,
    },
    error::CliError,
    storage::{AccountConfig, resolve_api_url},
    storage_auth::AccountAuth,
    util::{self, parse_user_ref_input},
};

use super::{context::CliContext, select::select_inline};

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
    Create(CreateAccountArgs),
    /// Login to an account (store authentication tokens)
    Login(LoginArgs),
    /// Logout from an account (remove authentication tokens)
    Logout(LogoutArgs),
    /// Show the active account, or another one, and its login status
    Status(StatusArgs),
    /// Delete an account (deletes the underlying user)
    Delete(DeleteAccountArgs),
    /// Switch the active account
    Switch(SwitchArgs),
}

#[derive(clap::Args, Debug)]
pub struct SwitchArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct CreateAccountArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
    /// Read the password from the first line of stdin
    // Name and host prompts would consume stdin
    #[clap(long, requires = "name")]
    pub password_stdin: bool,
}

#[derive(clap::Args, Clone, Debug)]
pub struct LoginArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
    /// Read the password from the first line of stdin
    #[clap(long)]
    pub password_stdin: bool,
    /// Make this the active account. A login always does when there is no
    /// active account.
    #[clap(long)]
    pub switch: bool,
}

#[derive(clap::Args, Clone, Debug)]
pub struct LogoutArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct StatusArgs {
    /// The account's username
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    /// The host name of the account's host
    #[clap(long, requires = "name")]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct DeleteAccountArgs {
    #[clap(long, requires = "host")]
    pub name: Option<String>,
    #[clap(long, requires = "name")]
    pub host: Option<String>,
    /// Delete without the confirmation prompt. Must match the account.
    #[clap(long, value_name = "NAME@HOST")]
    pub dangerously_confirm_delete: Option<String>,
}

/// An account in `account list --json`.
#[derive(Serialize)]
struct AccountListEntry<'a> {
    #[serde(flatten)]
    user_ref: &'a UserRef,
    active: bool,
}

/// The local authentication state of an account, for `--json`.
#[derive(Serialize)]
struct AccountStatus<'a> {
    #[serde(flatten)]
    user_ref: &'a UserRef,
    active: bool,
    /// Whether a refresh token is stored locally. The host may still reject
    /// it.
    authenticated: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    access_token_expires_at: Option<OffsetDateTime>,
    client_id: Option<&'a str>,
    scope: Option<&'a str>,
}

impl<'a> AccountStatus<'a> {
    fn new(
        user_ref: &'a UserRef,
        auth: Option<&'a AccountAuth>,
        active: bool,
    ) -> Self {
        Self {
            user_ref,
            active,
            authenticated: auth.is_some(),
            access_token_expires_at: auth
                .and_then(|auth| auth.expires_at)
                .and_then(|t| OffsetDateTime::from_unix_timestamp(t).ok()),
            client_id: auth.and_then(|auth| auth.client_id.as_deref()),
            scope: auth.and_then(|auth| auth.scope.as_deref()),
        }
    }
}

/// Prompts for the host and name of an account. With `--non-interactive`,
/// fails naming the flags that would provide them instead.
fn read_user_ref(strict: bool) -> Result<UserRef, CliError> {
    if is_non_interactive() {
        return Err(missing_argument("--name and --host"));
    }
    let read = |prompt: &str, field: &str| {
        read_input_preserving_whitespace(prompt)?.ok_or_else(|| {
            CliError::InvalidArgument(format!("{field} is required."))
        })
    };
    let host = read("Host: ", "Host")?;
    let name = read("Name: ", "Name")?;
    parse_user_ref_input(&name, &host, strict)
}

/// Chooses the account to log in to when there is no active account: a saved
/// account, or another one entered at prompts.
fn choose_login_account(ctx: &CliContext<'_>) -> Result<UserRef, CliError> {
    let accounts = &ctx.config.accounts;
    if accounts.is_empty() {
        return read_user_ref(ctx.strict_input);
    }
    let flags = "--name and --host";
    let choices = ["Saved account", "Another account"];
    let choice =
        select_inline(&choices, "Log in to", flags, |c| c.to_string())?
            .ok_or(CliError::Cancellation)?;
    eprintln!();
    if *choice == choices[1] {
        return read_user_ref(ctx.strict_input);
    }
    let account = select_inline(
        accounts,
        "Select account",
        flags,
        AccountConfig::to_string,
    )?
    .ok_or(CliError::Cancellation)?;
    eprintln!();
    Ok(account.user_ref.clone())
}

pub async fn handle_account_commands(
    ctx: &mut CliContext<'_>,
    command: &AccountCommands,
) -> Result<(), CliError> {
    match command {
        AccountCommands::List => {
            let accounts = &ctx.config.accounts;
            let entries: Vec<_> = accounts
                .iter()
                .map(|account| AccountListEntry {
                    user_ref: &account.user_ref,
                    active: ctx.config.is_active(&account.user_ref),
                })
                .collect();
            ctx.output(&entries, || {
                if accounts.is_empty() {
                    println!("No accounts.");
                }
                for account in accounts {
                    let prefix = util::get_prefix(
                        &account.user_ref,
                        ctx.config.active_account.as_ref(),
                        accounts.len(),
                    );
                    println!("{prefix}{account}");
                }
            })?;
        }

        AccountCommands::Create(create_args) => {
            let UserRef { name, host } =
                match (&create_args.name, &create_args.host) {
                    (Some(name), Some(host)) => {
                        parse_user_ref_input(name, host, ctx.strict_input)?
                    }
                    _ => read_user_ref(ctx.strict_input)?,
                };
            let api_url =
                resolve_api_url(ctx.client, ctx.config, &host).await?;
            let password = if create_args.password_stdin {
                read_password_stdin()?
            } else {
                read_password("Password: ")?.ok_or_else(|| {
                    CliError::InvalidArgument(
                        "Password is required. Pass --password-stdin to read \
                         it from stdin."
                            .into(),
                    )
                })?
            };
            let user_display = format!("{name}@{host}");
            let signup_req = SignupRequest {
                name,
                password: password.clone(),
            };
            let user =
                requests::auth::signup(ctx.client, &api_url, &signup_req)
                    .await
                    .map_err(|error| match error {
                        runelink_client::Error::Status(
                            StatusCode::CONFLICT,
                            _,
                        ) => CliError::InvalidArgument(format!(
                            "Name already taken: {user_display}"
                        )),
                        error => error.into(),
                    })?;
            let user_ref = user.as_ref();
            ctx.config.get_or_create_account_config(user_ref.clone());
            if ctx.config.accounts.len() == 1 {
                ctx.config.active_account = Some(user_ref.clone());
            }
            ctx.config.save()?;
            if let Err(error) =
                ctx.login_with_password(&user_ref, Some(password)).await
            {
                eprintln!(
                    "Created account: {user}, but logging in failed. Run \
                     `rune login --name {} --host {}` to retry.",
                    user_ref.name, user_ref.host
                );
                return Err(error);
            }
            ctx.output(&user, || {
                println!("Created account and logged in: {user}")
            })?;
        }

        AccountCommands::Login(login_args) => {
            // Get or discover account
            let user_ref = match (&login_args.name, &login_args.host) {
                (Some(name), Some(host)) => {
                    Some(parse_user_ref_input(name, host, ctx.strict_input)?)
                }
                _ if ctx.account.is_none() => Some(choose_login_account(ctx)?),
                _ => None,
            };
            let explicit_account = user_ref.is_some();
            let account = if let Some(user_ref) = user_ref {
                // Try to find existing account in config
                if let Some(acc) =
                    ctx.config.get_account_config(user_ref.clone())
                {
                    acc
                } else {
                    // Account doesn't exist, fetch it from server
                    let api_url =
                        resolve_api_url(ctx.client, ctx.config, &user_ref.host)
                            .await?;
                    let user = requests::users::fetch_by_ref(
                        ctx.client,
                        &api_url,
                        user_ref.clone(),
                    )
                    .await
                    .map_err(|error| match error {
                        runelink_client::Error::Status(
                            StatusCode::NOT_FOUND,
                            _,
                        ) => CliError::NotFound(format!(
                            "User not found: {user_ref}"
                        )),
                        error => error.into(),
                    })?;
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

            // An explicit password replaces credentials that may be stale
            let already_logged_in = !login_args.password_stdin
                && ctx.auth_cache.has_auth(&account_user_ref);
            if !already_logged_in {
                if explicit_account {
                    eprintln!("Logging in to {account_display}");
                } else {
                    eprintln!(
                        "Logging in to {account_display} \
                         (use --name and --host to change)"
                    );
                }
                let password = if login_args.password_stdin {
                    Some(read_password_stdin()?)
                } else {
                    None
                };
                ctx.login_with_password(&account_user_ref, password).await?;
            }
            if login_args.switch || ctx.config.get_active_account().is_none() {
                ctx.config.active_account = Some(account_user_ref.clone());
                ctx.config.save()?;
            }

            let status = AccountStatus::new(
                &account_user_ref,
                ctx.auth_cache.get(&account_user_ref),
                ctx.config.is_active(&account_user_ref),
            );
            let message = if already_logged_in {
                "Already logged in"
            } else {
                "Logged in successfully"
            };
            ctx.output(&status, || println!("{message}: {account_display}"))?;
        }

        AccountCommands::Logout(logout_args) => {
            let account = if let (Some(name), Some(host)) =
                (&logout_args.name, &logout_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config
                    .get_account_config(user_ref.clone())
                    .ok_or_else(|| CliError::account_not_found(&user_ref))?
            } else {
                ctx.account.ok_or(CliError::MissingAccount)?
            };

            let message = if ctx.auth_cache.remove(&account.user_ref).is_some()
            {
                ctx.auth_cache.save()?;
                "Logged out successfully"
            } else {
                "Not logged in"
            };
            let status = AccountStatus::new(
                &account.user_ref,
                None,
                ctx.config.is_active(&account.user_ref),
            );
            ctx.output(&status, || println!("{message}: {account}"))?;
        }

        AccountCommands::Status(status_args) => {
            let account = if let (Some(name), Some(host)) =
                (&status_args.name, &status_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config
                    .get_account_config(user_ref.clone())
                    .ok_or_else(|| CliError::account_not_found(&user_ref))?
            } else {
                ctx.account.ok_or(CliError::MissingAccount)?
            };

            let status = AccountStatus::new(
                &account.user_ref,
                ctx.auth_cache.get(&account.user_ref),
                ctx.config.is_active(&account.user_ref),
            );
            ctx.output(&status, || {
                if status.active {
                    println!("Account: {account} (active)");
                } else {
                    println!("Account: {account}");
                }
                if !status.authenticated {
                    println!("  Authenticated: No");
                    return;
                }
                println!("  Authenticated: Yes");
                let now = OffsetDateTime::now_utc();
                match status.access_token_expires_at {
                    Some(expires) if expires > now => println!(
                        "  Access token expires in: {} seconds",
                        (expires - now).whole_seconds()
                    ),
                    Some(_) => println!("  Access token: Expired"),
                    None => println!("  Access token: Not cached"),
                }
                if let Some(client_id) = status.client_id {
                    println!("  Client ID: {client_id}");
                }
                if let Some(scope) = status.scope {
                    println!("  Scope: {scope}");
                }
            })?;
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
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config
                    .get_account_config(user_ref.clone())
                    .ok_or_else(|| CliError::account_not_found(&user_ref))?;
                user_ref
            } else {
                let account = select_inline(
                    &ctx.config.accounts,
                    "Select account to delete",
                    "--name and --host",
                    AccountConfig::to_string,
                )?
                .ok_or(CliError::Cancellation)?;
                eprintln!();
                account.user_ref.clone()
            };

            let host = strip_default_port(&user_ref.host);
            let accepted = [
                format!("{}@{host}", user_ref.name),
                format!("{}@{}", user_ref.name, pad_host(&host)),
            ];
            if let Some(confirmation) = &delete_args.dangerously_confirm_delete
            {
                if !accepted.contains(confirmation) {
                    return Err(CliError::InvalidArgument(format!(
                        "--dangerously-confirm-delete does not match the \
                         account: {}",
                        accepted[0]
                    )));
                }
            } else if is_non_interactive() {
                return Err(missing_argument(&format!(
                    "--dangerously-confirm-delete {}",
                    accepted[0]
                )));
            } else {
                eprintln!(
                    "This permanently deletes the account {user_ref} on its home host."
                );
                let input = read_input_preserving_whitespace(&format!(
                    "Type \"delete {}\" to confirm: ",
                    accepted[0]
                ))?;
                if !input
                    .as_deref()
                    .and_then(|input| input.strip_prefix("delete "))
                    .is_some_and(|input| accepted.iter().any(|a| a == input))
                {
                    return Err(CliError::Cancellation);
                }
            }

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
            if ctx.config.active_account.as_ref() == Some(&user_ref) {
                ctx.config.active_account = None;
            }
            ctx.config.save()?;
            ctx.auth_cache.save()?;

            ctx.message(format!("Deleted account/user: {user_ref}"));
        }

        AccountCommands::Switch(switch_args) => {
            let user_ref = if let (Some(name), Some(host)) =
                (&switch_args.name, &switch_args.host)
            {
                let user_ref =
                    parse_user_ref_input(name, host, ctx.strict_input)?;
                ctx.config
                    .get_account_config(user_ref.clone())
                    .ok_or_else(|| CliError::account_not_found(&user_ref))?;
                user_ref
            } else {
                if ctx.config.accounts.is_empty() {
                    return Err(CliError::InvalidArgument(
                        "No accounts in local config.".into(),
                    ));
                }
                let account = select_inline(
                    &ctx.config.accounts,
                    "Select account",
                    "--name and --host",
                    AccountConfig::to_string,
                )?
                .ok_or(CliError::Cancellation)?;
                eprintln!();
                account.user_ref.clone()
            };
            ctx.config.active_account = Some(user_ref.clone());
            ctx.config.save()?;
            let status = AccountStatus::new(
                &user_ref,
                ctx.auth_cache.get(&user_ref),
                true,
            );
            ctx.output(&status, || {
                println!("Switched to account: {user_ref}")
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn alice() -> UserRef {
        UserRef::new("alice".into(), "localhost".into())
    }

    #[test]
    fn account_status_json() {
        let user_ref = alice();
        let auth = AccountAuth {
            refresh_token: "refresh".into(),
            access_token: Some("access".into()),
            expires_at: Some(1_800_000_000),
            client_id: Some("client".into()),
            scope: None,
        };
        assert_eq!(
            serde_json::to_value(AccountStatus::new(
                &user_ref,
                Some(&auth),
                true
            ))
            .unwrap(),
            json!({
                "name": "alice",
                "host": "localhost",
                "active": true,
                "authenticated": true,
                "access_token_expires_at": "2027-01-15T08:00:00Z",
                "client_id": "client",
                "scope": null,
            })
        );
        assert_eq!(
            serde_json::to_value(AccountStatus::new(&user_ref, None, false))
                .unwrap(),
            json!({
                "name": "alice",
                "host": "localhost",
                "active": false,
                "authenticated": false,
                "access_token_expires_at": null,
                "client_id": null,
                "scope": null,
            })
        );
    }
}
