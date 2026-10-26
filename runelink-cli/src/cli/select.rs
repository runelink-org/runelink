use crossterm::{
    cursor::{Hide, MoveToColumn, MoveUp, Show},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::{Print, Stylize},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use runelink_client::requests;
use runelink_types::{
    channel::{ServerChannel, ServerChannelId, ServerChannelKind},
    server::{Server, ServerId},
};
use std::collections::HashSet;
use std::fmt;
use std::io::Write;

use crate::error::CliError;

use super::context::CliContext;

pub fn select_inline<'a, T, F>(
    items: &'a [T],
    prompt: &str,
    display: F,
) -> std::io::Result<Option<&'a T>>
where
    F: Fn(&T) -> String,
{
    select_tree_inline(items, prompt, |_| 0, |_| true, display)
}

/// Like `select_inline`, indenting items by their tree depth while choosing.
/// Unselectable items are shown dimmed and skipped by the cursor. The
/// confirmed item is shown without its indentation.
pub fn select_tree_inline<'a, T, D, S, F>(
    items: &'a [T],
    prompt: &str,
    depth: D,
    selectable: S,
    display: F,
) -> std::io::Result<Option<&'a T>>
where
    D: Fn(&T) -> usize,
    S: Fn(&T) -> bool,
    F: Fn(&T) -> String,
{
    let (Some(first), Some(last)) = (
        items.iter().position(&selectable),
        items.iter().rposition(&selectable),
    ) else {
        println!("(no items to select)");
        return Ok(None);
    };
    let mut stdout = std::io::stdout();
    let draw = |stdout: &mut std::io::Stdout, selected: usize| {
        for (i, item) in items.iter().enumerate() {
            let prefix = if i == selected { "> " } else { "  " };
            let row = indented(depth(item), display(item));
            let row = if selectable(item) {
                row.stylize()
            } else {
                row.dim()
            };
            execute!(
                stdout,
                Clear(ClearType::CurrentLine),
                MoveToColumn(0),
                Print(format!("{prefix}{row}\n"))
            )?;
        }
        stdout.flush()
    };

    enable_raw_mode()?;
    execute!(
        stdout,
        Hide,
        MoveToColumn(0),
        Print(format!("{}\n", prompt))
    )?;
    let mut selected = first;
    draw(&mut stdout, selected)?;
    loop {
        if let Event::Key(KeyEvent {
            kind: KeyEventKind::Press,
            code,
            modifiers,
            ..
        }) = crossterm::event::read()?
        {
            match (code, modifiers) {
                // Ctrl-C - propagate as Interrupted
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    execute!(stdout, MoveToColumn(0))?;
                    disable_raw_mode()?;
                    execute!(stdout, Show)?;
                    panic!("Interrupted by Ctrl-C");
                }
                // Esc or 'q' - cancel
                (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                    execute!(stdout, MoveToColumn(0))?;
                    disable_raw_mode()?;
                    execute!(stdout, Show)?;
                    return Ok(None);
                }
                // Enter - confirm
                (KeyCode::Enter, _) => {
                    execute!(
                        stdout,
                        MoveUp(items.len() as u16),
                        MoveToColumn(0),
                        Clear(ClearType::FromCursorDown),
                        Print(format!("> {}\n", display(&items[selected]))),
                        MoveToColumn(0),
                    )?;
                    stdout.flush()?;
                    disable_raw_mode()?;
                    execute!(stdout, Show)?;
                    return Ok(Some(&items[selected]));
                }
                // Up or 'k'
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                    if let Some(i) =
                        items[..selected].iter().rposition(&selectable)
                    {
                        selected = i;
                    }
                }
                // Down or 'j'
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                    if let Some(i) =
                        items[selected + 1..].iter().position(&selectable)
                    {
                        selected += 1 + i;
                    }
                }
                (KeyCode::Char('g'), KeyModifiers::NONE)
                | (KeyCode::Home, _) => {
                    selected = first;
                }
                (KeyCode::Char('g'), KeyModifiers::SHIFT)
                | (KeyCode::Char('G'), _)
                | (KeyCode::End, _) => {
                    selected = last;
                }
                _ => {}
            }

            // Redraw the list in place
            execute!(stdout, MoveUp(items.len() as u16))?;
            draw(&mut stdout, selected)?;
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum ServerSelectionType<'a> {
    MemberOnly,
    All { host: &'a str },
    NonMemberOnly { host: &'a str },
}

pub async fn get_server_selection(
    ctx: &mut CliContext<'_>,
    selection_type: ServerSelectionType<'_>,
) -> Result<Server, CliError> {
    let api_url = ctx.home_api_url().await?;
    let account = ctx.account.ok_or(CliError::MissingAccount)?;
    let servers = match selection_type {
        ServerSelectionType::All { host } => {
            // Use target_host if it's different from account host
            let target_host = if account.user_ref.host != host {
                Some(host)
            } else {
                None
            };
            requests::servers::fetch_all(ctx.client, &api_url, target_host)
                .await?
        }

        ServerSelectionType::MemberOnly => {
            requests::servers::fetch_by_user(
                ctx.client,
                &api_url,
                account.user_ref.clone(),
            )
            .await?
        }

        ServerSelectionType::NonMemberOnly { host } => {
            // Fetch all servers from the specified host
            let target_host = if Some(host)
                != ctx.account.map(|ac| ac.user_ref.host.as_str())
            {
                Some(host)
            } else {
                None
            };
            let (all_servers_result, member_servers_result) = tokio::join!(
                requests::servers::fetch_all(ctx.client, &api_url, target_host),
                requests::servers::fetch_by_user(
                    ctx.client,
                    &api_url,
                    account.user_ref.clone(),
                )
            );
            let all_servers = all_servers_result?;
            let member_servers = member_servers_result?;
            // Create a set of member server IDs for efficient lookup
            let member_server_ids =
                member_servers.iter().map(|s| s.id).collect::<HashSet<_>>();
            // Filter out servers the user is already a member of
            all_servers
                .into_iter()
                .filter(|s| !member_server_ids.contains(&s.id))
                .collect()
        }
    };

    if servers.is_empty() {
        return Err(CliError::NoActionPossible(format!(
            "No applicable servers (viewing {:?}).\n\
                    For more information, try `rune server --help`.",
            selection_type,
        )));
    }
    let server = select_inline(&servers, "Select server", Server::to_string)?
        .ok_or(CliError::Cancellation)?;
    println!();
    Ok(server.clone())
}

#[derive(Debug, Clone)]
pub struct ChannelSelection {
    pub host: String,
    pub channel: ServerChannel,
}

/// Order channels depth-first through the category tree, with each
/// channel's depth.
pub fn tree_order(channels: &[ServerChannel]) -> Vec<(usize, &ServerChannel)> {
    let mut sorted: Vec<_> = channels.iter().collect();
    sorted.sort_by_key(|c| (c.position, c.id.as_uuid()));
    let mut ordered = Vec::with_capacity(channels.len());
    let mut stack: Vec<_> = sorted
        .iter()
        .rev()
        .filter(|c| c.parent_channel_id.is_none())
        .map(|c| (0, *c))
        .collect();
    while let Some((depth, channel)) = stack.pop() {
        ordered.push((depth, channel));
        stack.extend(
            sorted
                .iter()
                .rev()
                .filter(|c| c.parent_channel_id == Some(channel.id))
                .map(|c| (depth + 1, *c)),
        );
    }
    ordered
}

/// Display an item indented by its tree depth.
pub fn indented(depth: usize, item: impl fmt::Display) -> String {
    format!("{}{item}", "  ".repeat(depth))
}

/// Interactively select a channel, optionally only text channels.
pub async fn get_channel_selection(
    ctx: &mut CliContext<'_>,
    server_id: ServerId,
    server_host: &str,
    text_only: bool,
) -> Result<ChannelSelection, CliError> {
    let api_url = ctx.home_api_url().await?;
    let access_token = ctx.get_access_token().await?;
    let channels = requests::channels::fetch_by_server(
        ctx.client,
        &api_url,
        &access_token,
        server_id,
        Some(server_host),
    )
    .await?;
    // Categories stay visible for context when only text channels can be
    // chosen
    let selectable = |c: &ServerChannel| {
        !text_only || c.content.kind() == ServerChannelKind::Text
    };
    let choices = tree_order(&channels);
    if !choices.iter().any(|(_, c)| selectable(c)) {
        return Err(CliError::NoActionPossible(
            "No channels available.\n\
                For more information, try `rune channel --help`."
                .into(),
        ));
    }
    let prompt = if text_only {
        "Select channel"
    } else {
        "Select channel or category"
    };
    let (_, channel) = select_tree_inline(
        &choices,
        prompt,
        |(depth, _)| *depth,
        |(_, c)| selectable(c),
        |(_, c)| c.to_string(),
    )?
    .ok_or(CliError::Cancellation)?;
    println!();
    Ok(ChannelSelection {
        host: server_host.to_string(),
        channel: (*channel).clone(),
    })
}

/// Resolve a channel by ID, or prompt for one in the given server, or in a
/// server chosen at a prompt. Prompts offer only text channels when
/// `text_only` is set.
pub async fn get_channel_selection_with_inputs(
    ctx: &mut CliContext<'_>,
    channel_id: Option<ServerChannelId>,
    server_id: Option<ServerId>,
    host: Option<&str>,
    text_only: bool,
) -> Result<ChannelSelection, CliError> {
    let host = match host {
        Some(host) => host.to_string(),
        None => ctx.home_host()?.to_string(),
    };
    match (channel_id, server_id) {
        (Some(channel_id), _) => {
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let channel = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                channel_id,
                Some(host.as_str()),
            )
            .await?;
            Ok(ChannelSelection { host, channel })
        }
        (None, Some(server_id)) => {
            get_channel_selection(ctx, server_id, host.as_str(), text_only)
                .await
        }
        (None, None) => {
            let server =
                get_server_selection(ctx, ServerSelectionType::MemberOnly)
                    .await?;
            get_channel_selection(
                ctx,
                server.id,
                server.host.as_str(),
                text_only,
            )
            .await
        }
    }
}
