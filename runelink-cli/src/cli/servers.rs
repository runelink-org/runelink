#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

use runelink_client::requests;
use runelink_types::server::{
    NewServer, NewServerMembership, ServerId, ServerRole,
};

use crate::{error::CliError, util::group_memberships_by_host};

use super::{
    context::CliContext,
    input::{read_input, unwrap_or_prompt},
    select::{ServerSelectionType, get_server_selection},
};

#[derive(clap::Args, Debug)]
pub struct ServerArgs {
    #[clap(subcommand)]
    pub command: ServerCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum ServerCommands {
    /// List all servers
    List(ServerListArgs),
    /// Get a server by ID
    Get(ServerGetArg),
    /// Create a new server
    Create(ServerCreateArgs),
    /// Create a new server
    Join(ServerJoinArgs),
    /// Leave a server
    Leave(ServerLeaveArgs),
    /// Delete a server
    Delete(ServerDeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct ServerListArgs {
    /// The host to list servers from (if not provided, lists servers the user is a member of)
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServerGetArg {
    /// The ID of the server
    #[clap(long)]
    pub server_id: ServerId,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServerCreateArgs {
    /// The title of the server
    #[clap(long)]
    pub title: Option<String>,
    /// The description of the server
    #[clap(long)]
    pub description: Option<String>,
    /// Skip description cli prompt
    #[clap(long)]
    pub no_description: bool,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServerJoinArgs {
    /// The ID of the server
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServerLeaveArgs {
    /// The ID of the server to leave
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServerDeleteArgs {
    /// The ID of the server to delete
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

pub async fn handle_server_commands(
    ctx: &mut CliContext<'_>,
    server_args: &ServerArgs,
) -> Result<(), CliError> {
    match &server_args.command {
        ServerCommands::List(list_args) => {
            let api_url = ctx.home_api_url()?;

            if let Some(host) = &list_args.host {
                // List all servers in the specified host
                let servers = requests::servers::fetch_all(
                    ctx.client,
                    &api_url,
                    Some(host.as_str()),
                )
                .await?;
                if servers.is_empty() {
                    println!("No servers found in host: {host}");
                } else {
                    for server in servers {
                        println!("{}", server.verbose());
                    }
                }
            } else {
                // List servers the user is a member of (current behavior)
                let account = ctx.account.ok_or(CliError::MissingAccount)?;
                let memberships = requests::memberships::fetch_by_user(
                    ctx.client,
                    &api_url,
                    account.user_ref.clone(),
                )
                .await?;
                if memberships.is_empty() {
                    println!(
                        "No servers joined.\n\
                        For more information, try `rune server --help`."
                    )
                }
                let mut is_first = true;
                for (host, memberships) in
                    group_memberships_by_host(&memberships)
                {
                    if is_first {
                        is_first = false
                    } else {
                        println!(); // separation between host groups
                    }
                    println!("{host}");
                    for membership in memberships {
                        let server = &membership.server;
                        print!("    {}", server.verbose());
                        if membership.role == ServerRole::Admin {
                            println!(" - admin");
                        } else {
                            println!();
                        }
                    }
                }
            }
        }

        ServerCommands::Get(get_args) => {
            let api_url = ctx.home_api_url()?;
            let server = requests::servers::fetch_by_id(
                ctx.client,
                &api_url,
                get_args.server_id,
                get_args.host.as_deref(),
            )
            .await?;
            println!(
                "{host} / {title} ({id})",
                host = server.host,
                title = server.title,
                id = server.id
            );
        }

        ServerCommands::Create(create_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let title =
                unwrap_or_prompt(create_args.title.clone(), "Server Title")?;
            let description = if create_args.description.is_some() {
                create_args.description.clone()
            } else if create_args.no_description {
                None
            } else {
                read_input("Server Description (leave blank for none):\n> ")?
            };
            let new_server = NewServer { title, description };
            let server = requests::servers::create(
                ctx.client,
                &api_url,
                &access_token,
                &new_server,
                create_args.host.as_deref(),
            )
            .await?;
            println!("Created server: {}", server.verbose());
        }

        ServerCommands::Join(join_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let server = if let Some(server_id) = join_args.server_id {
                requests::servers::fetch_by_id(
                    ctx.client,
                    &api_url,
                    server_id,
                    join_args.host.as_deref(),
                )
                .await?
            } else {
                let host = join_args
                    .host
                    .as_deref()
                    .unwrap_or(account.user_ref.host.as_str());
                get_server_selection(
                    ctx,
                    ServerSelectionType::NonMemberOnly { host },
                )
                .await?
            };
            let new_member = NewServerMembership {
                user_ref: account.user_ref.clone(),
                server_id: server.id,
                server_host: server.host.clone(),
                role: ServerRole::Member,
            };
            let _member = requests::memberships::create(
                ctx.client,
                &api_url,
                &access_token,
                &new_member,
            )
            .await?;
            println!("Joined server: {}", server.verbose());
        }

        ServerCommands::Leave(leave_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let server = if let Some(server_id) = leave_args.server_id {
                requests::servers::fetch_by_id(
                    ctx.client,
                    &api_url,
                    server_id,
                    leave_args.host.as_deref(),
                )
                .await?
            } else {
                let host = leave_args
                    .host
                    .as_deref()
                    .unwrap_or(account.user_ref.host.as_str());
                get_server_selection(ctx, ServerSelectionType::MemberOnly)
                    .await?
            };
            requests::memberships::delete(
                ctx.client,
                &api_url,
                &access_token,
                server.id,
                account.user_ref.clone(),
                Some(server.host.as_str()),
            )
            .await?;
            println!("Left server: {}", server.verbose());
        }

        ServerCommands::Delete(delete_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let server_id = if let Some(server_id) = delete_args.server_id {
                server_id
            } else {
                let host = delete_args
                    .host
                    .as_deref()
                    .unwrap_or(account.user_ref.host.as_str());
                get_server_selection(ctx, ServerSelectionType::MemberOnly)
                    .await?
                    .id
            };
            requests::servers::delete(
                ctx.client,
                &api_url,
                &access_token,
                server_id,
                delete_args.host.as_deref(),
            )
            .await?;
            println!("Deleted server: {server_id}");
        }
    }
    Ok(())
}
