use runelink_client::requests;
use runelink_types::NewChannel;
use uuid::Uuid;

use crate::{
    cli::select::{
        ServerSelectionType, get_channel_selection_with_inputs,
        get_server_selection,
    },
    error::CliError,
};

use super::{
    context::CliContext,
    input::{read_input, unwrap_or_prompt},
};

#[derive(clap::Args, Debug)]
pub struct ChannelArgs {
    #[clap(subcommand)]
    pub command: ChannelCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum ChannelCommands {
    /// List all channels
    List(ChannelListArgs),
    /// Get a channel by ID
    Get(ChannelGetArgs),
    /// Create a new channel
    Create(ChannelCreateArgs),
    /// Delete a channel
    Delete(ChannelDeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct ChannelListArgs {
    /// Optional: Filter channels by Server ID
    #[clap(long)]
    pub server_id: Option<Uuid>,
    /// Include channels from all servers you are a member of
    #[clap(short, long)]
    pub all: bool,
    /// The host of host or server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelGetArgs {
    /// The ID of the server
    #[clap(long)]
    pub server_id: Uuid,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
    /// The ID of the channel
    #[clap(long)]
    pub channel_id: Uuid,
}

#[derive(clap::Args, Debug)]
pub struct ChannelCreateArgs {
    /// The title of the channel
    #[clap(long)]
    pub title: Option<String>,
    /// The description of the channel
    #[clap(long)]
    pub description: Option<String>,
    /// Skip description cli prompt
    #[clap(long)]
    pub no_description: bool,
    /// The server ID
    #[clap(long)]
    pub server_id: Option<Uuid>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelDeleteArgs {
    /// The ID of the server
    #[clap(long)]
    pub server_id: Option<Uuid>,
    /// The ID of the channel to delete
    #[clap(long)]
    pub channel_id: Option<Uuid>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

pub async fn handle_channel_commands(
    ctx: &mut CliContext<'_>,
    channel_args: &ChannelArgs,
) -> Result<(), CliError> {
    match &channel_args.command {
        ChannelCommands::List(list_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let channels = match (list_args.server_id, list_args.all) {
                (Some(_server_id), true) => {
                    return Err(CliError::InvalidArgument(
                        "Cannot use --all with --server-id.".into(),
                    ));
                }
                (Some(server_id), false) => {
                    requests::channels::fetch_by_server(
                        ctx.client,
                        &api_url,
                        &access_token,
                        server_id,
                        list_args.host.as_deref(),
                    )
                    .await?
                }
                (None, true) => {
                    requests::channels::fetch_all(
                        ctx.client,
                        &api_url,
                        &access_token,
                        list_args.host.as_deref(),
                    )
                    .await?
                }
                (None, false) => {
                    let server = get_server_selection(
                        ctx,
                        ServerSelectionType::MemberOnly,
                    )
                    .await?;
                    requests::channels::fetch_by_server(
                        ctx.client,
                        &api_url,
                        &access_token,
                        server.id,
                        Some(server.host.as_str()),
                    )
                    .await?
                }
            };
            if channels.is_empty() {
                println!(
                    "No channels available.\n\
                    For more information, try `rune channel --help`."
                )
            }
            for channel in channels {
                println!("{}", channel.verbose());
            }
        }

        ChannelCommands::Get(get_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let channel = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                get_args.server_id,
                get_args.channel_id,
                get_args.host.as_deref(),
            )
            .await?;
            println!("{}", channel.verbose());
        }

        ChannelCommands::Create(create_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let server = match create_args.server_id {
                Some(server_id) => {
                    requests::servers::fetch_by_id(
                        ctx.client,
                        &api_url,
                        server_id,
                        create_args.host.as_deref(),
                    )
                    .await?
                }
                None => {
                    get_server_selection(ctx, ServerSelectionType::MemberOnly)
                        .await?
                }
            };
            let title =
                unwrap_or_prompt(create_args.title.clone(), "Channel Title")?;
            let desc = if create_args.description.is_some() {
                create_args.description.clone()
            } else if create_args.no_description {
                None
            } else {
                read_input("Channel Description (leave blank for none):\n> ")?
            };
            let new_channel = NewChannel {
                title,
                description: desc,
            };
            let target_host = if server.host != account.user_ref.host {
                Some(server.host.as_str())
            } else {
                None
            };
            let channel = requests::channels::create(
                ctx.client,
                &api_url,
                &access_token,
                server.id,
                &new_channel,
                target_host,
            )
            .await?;
            println!("Created channel: {}", channel.verbose());
        }

        ChannelCommands::Delete(delete_args) => {
            let _account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url()?;
            let access_token = ctx.get_access_token().await?;
            let (server_id, channel_id, server_host) =
                match (delete_args.server_id, delete_args.channel_id) {
                    (Some(server_id), Some(channel_id)) => {
                        // Both IDs provided, use them directly
                        (server_id, channel_id, None)
                    }
                    _ => {
                        // Need to select server and/or channel
                        let (server, channel) =
                            get_channel_selection_with_inputs(
                                ctx,
                                delete_args.channel_id,
                                delete_args.server_id,
                                delete_args.host.as_deref(),
                            )
                            .await?;
                        (server.id, channel.id, Some(server.host.clone()))
                    }
                };
            let target_host = server_host
                .as_deref()
                .or_else(|| delete_args.host.as_deref());
            requests::channels::delete(
                ctx.client,
                &api_url,
                &access_token,
                server_id,
                channel_id,
                target_host,
            )
            .await?;
            println!("Deleted channel: {channel_id}");
        }
    };
    Ok(())
}
