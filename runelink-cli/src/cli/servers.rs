use runelink_client::requests;
use runelink_types::server::{
    NewServer, NewServerMembership, ServerId, ServerRef, ServerRole,
};

use crate::util::{parse_optional_host_input, server_label};
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
    /// Edit an existing server
    Edit(ServerEditArgs),
    /// List all servers
    List(ServerListArgs),
    /// Get a server by ID
    Get(ServerGetArg),
    /// Create a new server
    Create(ServerCreateArgs),
    /// Join a server
    Join(ServerJoinArgs),
    /// Leave a server
    Leave(ServerLeaveArgs),
    /// Delete a server
    Delete(ServerDeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct ServerListArgs {
    /// Show the full server ID
    #[clap(long, conflicts_with = "hide_ids")]
    pub show_ids: bool,
    /// Hide IDs (the default)
    #[clap(long)]
    pub hide_ids: bool,
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
    /// Don't create the default "general" text channel
    #[clap(long)]
    pub no_default_channels: bool,
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
        ServerCommands::Edit(edit_args) => {
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let server_ref = ServerRef::new(
                ctx.resource_host(edit_args.host.as_deref())?,
                edit_args.server_id,
            );
            let original = requests::servers::fetch_by_id(
                ctx.client,
                &api_url,
                &server_ref,
            )
            .await?;
            let update = runelink_types::edit::DetailsUpdate {
                title: edit_args.title.clone(),
                description: if edit_args.clear_description {
                    Some(None)
                } else {
                    edit_args.description.clone().map(Some)
                },
                expected_updated_at: original.updated_at,
            };
            let resource = requests::servers::update(
                ctx.client,
                &api_url,
                &access_token,
                &server_ref,
                &update,
            )
            .await?;
            ctx.output(&resource, || println!("Updated server: {resource}"))?;
        }

        ServerCommands::List(list_args) => {
            let show_ids = list_args.show_ids && !list_args.hide_ids;
            let api_url = ctx.home_api_url().await?;
            let target_host = parse_optional_host_input(
                list_args.host.as_deref(),
                ctx.strict_input,
            )?;

            if let Some(host) = &target_host {
                // List all servers in the specified host
                let servers = requests::servers::fetch_all(
                    ctx.client,
                    &api_url,
                    Some(host),
                )
                .await?;
                ctx.output(&servers, || {
                    if servers.is_empty() {
                        println!("No servers found in host: {host}");
                    }
                    for server in &servers {
                        println!(
                            "{}",
                            server_label(
                                server,
                                show_ids,
                                ctx.show_server_hosts
                            )
                        );
                    }
                })?;
            } else {
                // List servers the user is a member of (current behavior)
                let account = ctx.account.ok_or(CliError::MissingAccount)?;
                let memberships = requests::memberships::fetch_by_user(
                    ctx.client,
                    &api_url,
                    account.user_ref.clone(),
                )
                .await?;
                ctx.output(&memberships, || {
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
                            print!(
                                "    {}",
                                server_label(
                                    server,
                                    show_ids,
                                    ctx.show_server_hosts
                                )
                            );
                            if membership.role == ServerRole::Admin {
                                println!(" - admin");
                            } else {
                                println!();
                            }
                        }
                    }
                })?;
            }
        }

        ServerCommands::Get(get_args) => {
            let api_url = ctx.home_api_url().await?;
            let server_ref = ServerRef::new(
                ctx.resource_host(get_args.host.as_deref())?,
                get_args.server_id,
            );
            let server = requests::servers::fetch_by_id(
                ctx.client,
                &api_url,
                &server_ref,
            )
            .await?;
            ctx.output(&server, || {
                println!(
                    "{host} / {title} ({id})",
                    host = server.host,
                    title = server.title,
                    id = server.id
                )
            })?;
        }

        ServerCommands::Create(create_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                create_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let title = unwrap_or_prompt(
                create_args.title.clone(),
                "Server Title",
                "--title",
            )?;
            let description = if create_args.description.is_some() {
                create_args.description.clone()
            } else if create_args.no_description {
                None
            } else {
                read_input("Server Description (leave blank for none):\n> ")?
            };
            let new_server = NewServer {
                title,
                description,
                default_channels: !create_args.no_default_channels,
            };
            let server = requests::servers::create(
                ctx.client,
                &api_url,
                &access_token,
                &new_server,
                target_host.as_ref(),
            )
            .await?;
            ctx.output(&server, || {
                println!("Created server: {}", server.verbose())
            })?;
        }

        ServerCommands::Join(join_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let host = ctx.resource_host(join_args.host.as_deref())?;
            let server = if let Some(server_id) = join_args.server_id {
                requests::servers::fetch_by_id(
                    ctx.client,
                    &api_url,
                    &ServerRef::new(host, server_id),
                )
                .await?
            } else {
                get_server_selection(
                    ctx,
                    ServerSelectionType::NonMemberOnly { host: &host },
                    "--server-id",
                )
                .await?
            };
            let new_member = NewServerMembership {
                user_ref: account.user_ref.clone(),
                server_ref: server.to_ref(),
                role: ServerRole::Member,
            };
            let membership = requests::memberships::create(
                ctx.client,
                &api_url,
                &access_token,
                &new_member,
            )
            .await?;
            ctx.output(&membership, || {
                println!("Joined server: {}", server.verbose())
            })?;
        }

        ServerCommands::Leave(leave_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let server = if let Some(server_id) = leave_args.server_id {
                let host = ctx.resource_host(leave_args.host.as_deref())?;
                requests::servers::fetch_by_id(
                    ctx.client,
                    &api_url,
                    &ServerRef::new(host, server_id),
                )
                .await?
            } else {
                get_server_selection(
                    ctx,
                    ServerSelectionType::MemberOnly,
                    "--server-id",
                )
                .await?
            };
            requests::memberships::delete(
                ctx.client,
                &api_url,
                &access_token,
                &server.to_ref(),
                account.user_ref.clone(),
            )
            .await?;
            ctx.message(format!("Left server: {}", server.verbose()));
        }

        ServerCommands::Delete(delete_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let server_ref = if let Some(server_id) = delete_args.server_id {
                ServerRef::new(
                    ctx.resource_host(delete_args.host.as_deref())?,
                    server_id,
                )
            } else {
                get_server_selection(
                    ctx,
                    ServerSelectionType::MemberOnly,
                    "--server-id",
                )
                .await?
                .to_ref()
            };
            requests::servers::delete(
                ctx.client,
                &api_url,
                &access_token,
                &server_ref,
            )
            .await?;
            ctx.message(format!("Deleted server: {}", server_ref.id));
        }
    }
    Ok(())
}

#[derive(clap::Args, Debug)]
pub struct ServerEditArgs {
    #[clap(long)]
    pub server_id: ServerId,
    #[clap(long)]
    pub host: Option<String>,
    /// New title
    #[clap(long)]
    pub title: Option<String>,
    /// New description
    #[clap(long, conflicts_with = "clear_description")]
    pub description: Option<String>,
    /// Remove the description
    #[clap(long)]
    pub clear_description: bool,
}
