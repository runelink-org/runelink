use runelink_client::requests;
use runelink_types::{
    channel::{
        NewServerChannel, ServerChannelId, ServerChannelKind, ServerChannelMove,
    },
    server::ServerId,
};

use crate::{
    cli::select::{
        ServerSelectionType, get_channel_selection_with_inputs,
        get_server_selection, indented, select_inline, tree_order,
    },
    error::CliError,
    util::parse_optional_host_input,
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
    /// Delete a channel or an empty category
    Delete(ChannelDeleteArgs),
    /// Move a channel or category within its server
    Move(ChannelMoveArgs),
}

#[derive(clap::Args, Debug)]
pub struct ChannelListArgs {
    /// Optional: Filter channels by Server ID
    #[clap(long)]
    pub server_id: Option<ServerId>,
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
    pub server_id: ServerId,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
    /// The ID of the channel
    #[clap(long)]
    pub channel_id: ServerChannelId,
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
    /// Create a category instead of a text channel
    #[clap(long)]
    pub category: bool,
    /// The parent category ID (defaults to the server root)
    #[clap(long)]
    pub parent_id: Option<ServerChannelId>,
    /// The server ID
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelDeleteArgs {
    /// The ID of the server
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The ID of the channel to delete
    #[clap(long)]
    pub channel_id: Option<ServerChannelId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelMoveArgs {
    /// The ID of the server
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The ID of the channel to move
    #[clap(long)]
    pub channel_id: Option<ServerChannelId>,
    /// The new parent category ID
    #[clap(long, conflicts_with = "root")]
    pub parent_id: Option<ServerChannelId>,
    /// Move to the server root
    #[clap(long)]
    pub root: bool,
    /// The new index among its siblings, starting from 0
    #[clap(long)]
    pub index: Option<u32>,
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
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                list_args.host.as_deref(),
                ctx.strict_input,
            )?;
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
                        target_host.as_deref(),
                    )
                    .await?
                }
                (None, true) => {
                    requests::channels::fetch_all(
                        ctx.client,
                        &api_url,
                        &access_token,
                        target_host.as_deref(),
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
            for (depth, channel) in tree_order(&channels) {
                println!("{}  ({})", indented(depth, channel), channel.id);
            }
        }

        ChannelCommands::Get(get_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                get_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let channel = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                get_args.server_id,
                get_args.channel_id,
                target_host.as_deref(),
            )
            .await?;
            println!("{}", channel.verbose());
        }

        ChannelCommands::Create(create_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                create_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let server = match create_args.server_id {
                Some(server_id) => {
                    requests::servers::fetch_by_id(
                        ctx.client,
                        &api_url,
                        server_id,
                        target_host.as_deref(),
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
            let kind = if create_args.category {
                ServerChannelKind::Category
            } else {
                ServerChannelKind::Text
            };
            let new_channel = NewServerChannel {
                kind,
                title,
                description: desc,
                parent_channel_id: create_args.parent_id,
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
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                delete_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let selection = get_channel_selection_with_inputs(
                ctx,
                delete_args.channel_id,
                delete_args.server_id,
                target_host.as_deref(),
                false,
            )
            .await?;
            let channel = &selection.channel;
            requests::channels::delete(
                ctx.client,
                &api_url,
                &access_token,
                channel.server_id,
                channel.id,
                Some(&selection.host),
            )
            .await?;
            println!("Deleted channel: {}", channel.verbose());
        }

        ChannelCommands::Move(move_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let target_host = parse_optional_host_input(
                move_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let selection = get_channel_selection_with_inputs(
                ctx,
                move_args.channel_id,
                move_args.server_id,
                target_host.as_deref(),
                false,
            )
            .await?;
            let channel = &selection.channel;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let parent_channel_id = match (move_args.parent_id, move_args.root)
            {
                (Some(parent_id), _) => Some(parent_id),
                (None, true) => None,
                (None, false) => {
                    let channels = requests::channels::fetch_by_server(
                        ctx.client,
                        &api_url,
                        &access_token,
                        channel.server_id,
                        Some(&selection.host),
                    )
                    .await?;
                    // Offer categories outside the moved channel's subtree,
                    // which follows it in tree order at a greater depth
                    let mut parents = vec![(0, None)];
                    let mut subtree_depth = None;
                    for (depth, c) in tree_order(&channels) {
                        if subtree_depth.is_some_and(|d| depth > d) {
                            continue;
                        }
                        subtree_depth = (c.id == channel.id).then_some(depth);
                        if subtree_depth.is_none()
                            && c.kind == ServerChannelKind::Category
                        {
                            parents.push((depth + 1, Some(c)));
                        }
                    }
                    let (_, parent) =
                        select_inline(&parents, "Select parent", |entry| {
                            match entry {
                                (_, None) => "(server root)".into(),
                                (depth, Some(c)) => indented(*depth, c),
                            }
                        })?
                        .ok_or(CliError::Cancellation)?;
                    println!();
                    parent.map(|c| c.id)
                }
            };
            let index = unwrap_or_prompt(
                move_args.index,
                "Index among siblings (0 is first)",
            )?;
            let changed = requests::channels::move_channel(
                ctx.client,
                &api_url,
                &access_token,
                channel.server_id,
                channel.id,
                &ServerChannelMove {
                    parent_channel_id,
                    index,
                },
                Some(&selection.host),
            )
            .await?;
            if changed.is_empty() {
                println!("Channel already in place: {}", channel.verbose());
            } else {
                println!("Moved channel: {}", channel.verbose());
            }
        }
    };
    Ok(())
}
