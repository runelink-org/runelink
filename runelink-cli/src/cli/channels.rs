use runelink_client::requests;
use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelKind,
        ServerChannelMove, ServerChannelRef,
    },
    server::{ServerId, ServerRef},
};

use crate::{
    cli::select::{
        ServerSelectionType, get_channel_selection_with_inputs,
        get_server_selection, indented, select_inline, select_tree_inline,
        tree_order,
    },
    error::CliError,
    util::{list_label, parse_optional_host_input},
};

use super::{
    context::CliContext,
    input::{read_input, unwrap_or_prompt},
};

// The word for a server channel of this kind in prompts and output
fn label(kind: ServerChannelKind) -> &'static str {
    match kind {
        ServerChannelKind::Text => "Channel",
        ServerChannelKind::Category => "Category",
    }
}

// A sibling index, with -1 for last. The host places indices past the end
// last.
fn parse_index(input: &str) -> Result<u32, String> {
    match input {
        "-1" => Ok(u32::MAX),
        _ => input.parse().map_err(|_| {
            format!("Invalid index {input:?}, expected 0 or more, or -1.")
        }),
    }
}

// Prompt for a parent among the server root and the categories outside
// `excluded`'s subtree, skipping the prompt when only the root remains
fn select_parent(
    channels: &[ServerChannel],
    excluded: Option<ServerChannelId>,
) -> Result<Option<ServerChannelId>, CliError> {
    // The excluded subtree follows it in tree order at a greater depth
    let mut parents = vec![(0, None)];
    let mut subtree_depth = None;
    for (depth, c) in tree_order(channels) {
        if subtree_depth.is_some_and(|d| depth > d) {
            continue;
        }
        subtree_depth = (Some(c.id) == excluded).then_some(depth);
        if subtree_depth.is_none() && c.is_category() {
            parents.push((depth + 1, Some(c)));
        }
    }
    if parents.len() == 1 {
        return Ok(None);
    }
    let (_, parent) = select_tree_inline(
        &parents,
        "Select location",
        "--parent-id or --root",
        |(depth, _)| *depth,
        |_| true,
        |(_, parent)| match parent {
            None => "(server root)".into(),
            Some(c) => c.to_string(),
        },
    )?
    .ok_or(CliError::Cancellation)?;
    eprintln!();
    Ok(parent.map(|c| c.id))
}

#[derive(clap::Args, Debug)]
pub struct ChannelArgs {
    #[clap(subcommand)]
    pub command: ChannelCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum ChannelCommands {
    /// Edit an existing channel
    Edit(ChannelEditArgs),
    /// List all channels and categories
    List(ChannelListArgs),
    /// Get a channel or category by ID
    Get(ChannelGetArgs),
    /// Create a new channel or category
    Create(ChannelCreateArgs),
    /// Delete a channel or an empty category
    Delete(ChannelDeleteArgs),
    /// Move a channel or category within its server
    Move(ChannelMoveArgs),
}

#[derive(clap::Args, Debug)]
pub struct ChannelListArgs {
    /// Show the full channel or category ID
    #[clap(long, conflicts_with = "hide_ids")]
    pub show_ids: bool,
    /// Show the ID of the underlying channel, such as the text channel ID
    /// of a text channel. Categories have none.
    #[clap(long, conflicts_with = "hide_ids")]
    pub show_raw_ids: bool,
    /// Hide IDs (the default)
    #[clap(long)]
    pub hide_ids: bool,
    /// Optional: Filter channels and categories by Server ID
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// Include channels and categories from all servers you are a member of
    #[clap(short, long)]
    pub all: bool,
    /// The host of host or server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelGetArgs {
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
    /// The ID of the channel or category
    #[clap(long)]
    pub channel_id: ServerChannelId,
}

#[derive(clap::Args, Debug)]
pub struct ChannelCreateArgs {
    #[clap(flatten)]
    pub metadata: super::metadata::MetadataArgs,
    /// The title of the channel or category
    #[clap(long)]
    pub title: Option<String>,
    /// The description of the channel or category
    #[clap(long)]
    pub description: Option<String>,
    /// Skip description cli prompt
    #[clap(long)]
    pub no_description: bool,
    /// Create a text channel
    #[clap(long, conflicts_with = "category")]
    pub text: bool,
    /// Create a category
    #[clap(long)]
    pub category: bool,
    /// The parent category ID
    #[clap(long, conflicts_with = "root")]
    pub parent_id: Option<ServerChannelId>,
    /// Create at the server root
    #[clap(long)]
    pub root: bool,
    /// The server ID
    #[clap(long)]
    pub server_id: Option<ServerId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelDeleteArgs {
    /// The server to select a channel or category from
    #[clap(long, conflicts_with = "channel_id")]
    pub server_id: Option<ServerId>,
    /// The ID of the channel or category to delete
    #[clap(long)]
    pub channel_id: Option<ServerChannelId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ChannelMoveArgs {
    /// The server to select a channel or category from
    #[clap(long, conflicts_with = "channel_id")]
    pub server_id: Option<ServerId>,
    /// The ID of the channel or category to move
    #[clap(long)]
    pub channel_id: Option<ServerChannelId>,
    /// The new parent category ID
    #[clap(long, conflicts_with = "root")]
    pub parent_id: Option<ServerChannelId>,
    /// Move to the server root
    #[clap(long)]
    pub root: bool,
    /// The new index among its siblings, starting from 0, or -1 for last
    #[clap(long, allow_negative_numbers = true, value_parser = parse_index)]
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
        ChannelCommands::Edit(edit_args) => {
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let channel_ref = ServerChannelRef::new(
                ctx.resource_host(edit_args.host.as_deref())?,
                edit_args.channel_id,
            );
            let original = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                &channel_ref,
            )
            .await?;
            let update = runelink_types::channel::ServerChannelUpdate {
                title: edit_args.title.clone(),
                metadata: edit_args.metadata.patch()?,
                description: if edit_args.clear_description {
                    Some(None)
                } else {
                    edit_args.description.clone().map(Some)
                },
                expected_updated_at: original.updated_at,
            };
            let resource = requests::channels::update(
                ctx.client,
                &api_url,
                &access_token,
                &channel_ref,
                &update,
            )
            .await?;
            ctx.output(&resource, || println!("Updated channel: {resource}"))?;
        }

        ChannelCommands::List(list_args) => {
            let show_ids = list_args.show_ids && !list_args.hide_ids;
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
                    let host = match target_host {
                        Some(host) => host,
                        None => ctx.home_host()?.clone(),
                    };
                    requests::channels::fetch_by_server(
                        ctx.client,
                        &api_url,
                        &access_token,
                        &ServerRef::new(host, server_id),
                    )
                    .await?
                }
                (None, true) => {
                    requests::channels::fetch_all(
                        ctx.client,
                        &api_url,
                        &access_token,
                        target_host.as_ref(),
                    )
                    .await?
                }
                (None, false) => {
                    let server = get_server_selection(
                        ctx,
                        ServerSelectionType::MemberOnly,
                        "--server-id or --all",
                    )
                    .await?;
                    requests::channels::fetch_by_server(
                        ctx.client,
                        &api_url,
                        &access_token,
                        &server.to_ref(),
                    )
                    .await?
                }
            };
            let ordered = tree_order(&channels);
            let flat: Vec<_> = ordered.iter().map(|(_, c)| c).collect();
            ctx.output(&flat, || {
                if channels.is_empty() {
                    println!(
                        "No channels available.\n\
                        For more information, try `rune channel --help`."
                    )
                }
                for (depth, channel) in &ordered {
                    let mut label = list_label(
                        indented(*depth, channel),
                        channel.id,
                        show_ids,
                    );
                    if list_args.show_raw_ids
                        && let Some(raw_id) = channel.raw_id()
                    {
                        label = format!("{label} [{raw_id}]");
                    }
                    println!("{label}");
                }
            })?;
        }

        ChannelCommands::Get(get_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let channel_ref = ServerChannelRef::new(
                ctx.resource_host(get_args.host.as_deref())?,
                get_args.channel_id,
            );
            let channel = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                &channel_ref,
            )
            .await?;
            ctx.output(&channel, || println!("{}", channel.verbose()))?;
        }

        ChannelCommands::Create(create_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let server_ref = match create_args.server_id {
                Some(server_id) => ServerRef::new(
                    ctx.resource_host(create_args.host.as_deref())?,
                    server_id,
                ),
                None => get_server_selection(
                    ctx,
                    ServerSelectionType::MemberOnly,
                    "--server-id",
                )
                .await?
                .to_ref(),
            };
            let parent_channel_id =
                match (create_args.parent_id, create_args.root) {
                    (Some(parent_id), _) => Some(parent_id),
                    (None, true) => None,
                    (None, false) => {
                        let channels = requests::channels::fetch_by_server(
                            ctx.client,
                            &api_url,
                            &access_token,
                            &server_ref,
                        )
                        .await?;
                        select_parent(&channels, None)?
                    }
                };
            let kind = match (create_args.text, create_args.category) {
                (true, _) => ServerChannelKind::Text,
                (_, true) => ServerChannelKind::Category,
                _ => {
                    let kinds =
                        [ServerChannelKind::Text, ServerChannelKind::Category];
                    let kind = *select_inline(
                        &kinds,
                        "Select type",
                        "--text or --category",
                        |kind| match kind {
                            ServerChannelKind::Text => "Text".into(),
                            ServerChannelKind::Category => "Category".into(),
                        },
                    )?
                    .ok_or(CliError::Cancellation)?;
                    eprintln!();
                    kind
                }
            };
            let kind_label = label(kind);
            let title = unwrap_or_prompt(
                create_args.title.clone(),
                &format!("{kind_label} Title"),
                "--title",
            )?;
            let desc = if create_args.description.is_some() {
                create_args.description.clone()
            } else if create_args.no_description {
                None
            } else {
                read_input(&format!(
                    "{kind_label} Description (leave blank for none):\n> "
                ))?
            };
            let new_channel = NewServerChannel {
                kind,
                title,
                description: desc,
                metadata: create_args.metadata.value()?,
                parent_channel_id,
            };
            let channel = requests::channels::create(
                ctx.client,
                &api_url,
                &access_token,
                &server_ref,
                &new_channel,
            )
            .await?;
            ctx.output(&channel, || {
                println!(
                    "Created {}: {}",
                    label(channel.kind()).to_lowercase(),
                    channel.verbose()
                )
            })?;
        }

        ChannelCommands::Delete(delete_args) => {
            let _account = ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let channel = get_channel_selection_with_inputs(
                ctx,
                delete_args.channel_id,
                delete_args.server_id,
                delete_args.host.as_deref(),
                false,
                "--channel-id",
            )
            .await?;
            requests::channels::delete(
                ctx.client,
                &api_url,
                &access_token,
                &channel.to_ref(),
            )
            .await?;
            ctx.message(format!(
                "Deleted {}: {}",
                label(channel.kind()).to_lowercase(),
                channel.verbose()
            ));
        }

        ChannelCommands::Move(move_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let channel = get_channel_selection_with_inputs(
                ctx,
                move_args.channel_id,
                move_args.server_id,
                move_args.host.as_deref(),
                false,
                "--channel-id",
            )
            .await?;
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
                        &channel.server_ref(),
                    )
                    .await?;
                    select_parent(&channels, Some(channel.id))?
                }
            };
            let index = match move_args.index {
                Some(index) => index,
                None => {
                    read_input("Index among siblings (defaults to last): ")?
                        .map_or(Ok(u32::MAX), |input| parse_index(&input))
                        .map_err(CliError::InvalidArgument)?
                }
            };
            let changed = requests::channels::move_channel(
                ctx.client,
                &api_url,
                &access_token,
                &channel.to_ref(),
                &ServerChannelMove {
                    parent_channel_id,
                    index,
                },
            )
            .await?;
            ctx.output(&changed, || {
                if changed.is_empty() {
                    println!(
                        "{} already in place: {}",
                        label(channel.kind()),
                        channel.verbose()
                    );
                } else {
                    println!(
                        "Moved {}: {}",
                        label(channel.kind()).to_lowercase(),
                        channel.verbose()
                    );
                }
            })?;
        }
    };
    Ok(())
}

#[derive(clap::Args, Debug)]
pub struct ChannelEditArgs {
    #[clap(flatten)]
    pub metadata: super::metadata::MetadataEditArgs,
    #[clap(long)]
    pub channel_id: ServerChannelId,
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
