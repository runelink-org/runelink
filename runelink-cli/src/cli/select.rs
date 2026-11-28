use crossterm::{
    cursor::{Hide, MoveToColumn, MoveUp, Show},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::{Print, Stylize},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use runelink_client::requests;
use runelink_types::{
    channel::{ServerChannel, ServerChannelId, ServerChannelRef},
    host::Host,
    server::{Server, ServerId, ServerRef},
};
use std::collections::HashSet;
use std::fmt;
use std::io::Write;

use crate::{error::CliError, util::server_label};

use super::{
    context::CliContext,
    input::{is_non_interactive, missing_argument},
};

/// Interactively select an item. With `--non-interactive`, fails naming the
/// `flags` that would provide the choice instead.
pub fn select_inline<'a, T, F>(
    items: &'a [T],
    prompt: &str,
    flags: &str,
    display: F,
) -> Result<Option<&'a T>, CliError>
where
    F: Fn(&T) -> String,
{
    select_tree_inline(items, prompt, flags, |_| 0, |_| true, display)
}

/// Like `select_inline`, indenting items by their tree depth while choosing.
/// Unselectable items are shown dimmed and skipped by the cursor. The
/// confirmed item is shown without its indentation.
pub fn select_tree_inline<'a, T, D, S, F>(
    items: &'a [T],
    prompt: &str,
    flags: &str,
    depth: D,
    selectable: S,
    display: F,
) -> Result<Option<&'a T>, CliError>
where
    D: Fn(&T) -> usize,
    S: Fn(&T) -> bool,
    F: Fn(&T) -> String,
{
    if is_non_interactive() {
        return Err(missing_argument(flags));
    }
    let (Some(first), Some(last)) = (
        items.iter().position(&selectable),
        items.iter().rposition(&selectable),
    ) else {
        eprintln!("(no items to select)");
        return Ok(None);
    };
    let mut stderr = std::io::stderr();
    let draw = |stderr: &mut std::io::Stderr, selected: usize| {
        for (i, item) in items.iter().enumerate() {
            let prefix = if i == selected { "> " } else { "  " };
            let row = indented(depth(item), display(item));
            let row = if selectable(item) {
                row.stylize()
            } else {
                row.dim()
            };
            execute!(
                stderr,
                Clear(ClearType::CurrentLine),
                MoveToColumn(0),
                Print(format!("{prefix}{row}\n"))
            )?;
        }
        stderr.flush()
    };

    enable_raw_mode()?;
    execute!(
        stderr,
        Hide,
        MoveToColumn(0),
        Print(format!("{}\n", prompt))
    )?;
    let mut selected = first;
    draw(&mut stderr, selected)?;
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
                    execute!(stderr, MoveToColumn(0))?;
                    disable_raw_mode()?;
                    execute!(stderr, Show)?;
                    panic!("Interrupted by Ctrl-C");
                }
                // Esc or 'q' - cancel
                (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                    execute!(stderr, MoveToColumn(0))?;
                    disable_raw_mode()?;
                    execute!(stderr, Show)?;
                    return Ok(None);
                }
                // Enter - confirm
                (KeyCode::Enter, _) => {
                    execute!(
                        stderr,
                        MoveUp(items.len() as u16),
                        MoveToColumn(0),
                        Clear(ClearType::FromCursorDown),
                        Print(format!("> {}\n", display(&items[selected]))),
                        MoveToColumn(0),
                    )?;
                    stderr.flush()?;
                    disable_raw_mode()?;
                    execute!(stderr, Show)?;
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
            execute!(stderr, MoveUp(items.len() as u16))?;
            draw(&mut stderr, selected)?;
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum ServerSelectionType<'a> {
    MemberOnly,
    All { host: &'a Host },
    NonMemberOnly { host: &'a Host },
}

/// Interactively select a server. `flags` name the arguments that would
/// provide the choice instead.
pub async fn get_server_selection(
    ctx: &mut CliContext<'_>,
    selection_type: ServerSelectionType<'_>,
    flags: &str,
) -> Result<Server, CliError> {
    let api_url = ctx.home_api_url().await?;
    let account = ctx.account.ok_or(CliError::MissingAccount)?;
    let servers = match selection_type {
        ServerSelectionType::All { host } => {
            requests::servers::fetch_all(ctx.client, &api_url, Some(host))
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
            let (all_servers_result, member_servers_result) = tokio::join!(
                requests::servers::fetch_all(ctx.client, &api_url, Some(host)),
                requests::servers::fetch_by_user(
                    ctx.client,
                    &api_url,
                    account.user_ref.clone(),
                )
            );
            let all_servers = all_servers_result?;
            let member_servers = member_servers_result?;
            let member_servers = member_servers
                .iter()
                .map(Server::to_ref)
                .collect::<HashSet<_>>();
            // Filter out servers the user is already a member of
            all_servers
                .into_iter()
                .filter(|s| !member_servers.contains(&s.to_ref()))
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
    let server = select_inline(&servers, "Select server", flags, |server| {
        server_label(server, false, ctx.show_server_hosts)
    })?
    .ok_or(CliError::Cancellation)?;
    eprintln!();
    Ok(server.clone())
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

/// Interactively select a channel, optionally only text channels. `flags`
/// name the arguments that would provide the choice instead.
pub async fn get_channel_selection(
    ctx: &mut CliContext<'_>,
    server_ref: &ServerRef,
    text_only: bool,
    flags: &str,
) -> Result<ServerChannel, CliError> {
    let api_url = ctx.home_api_url().await?;
    let access_token = ctx.get_access_token().await?;
    let channels = requests::channels::fetch_by_server(
        ctx.client,
        &api_url,
        &access_token,
        server_ref,
    )
    .await?;
    // Categories stay visible for context when only text channels can be
    // chosen
    let selectable = |c: &ServerChannel| !text_only || c.is_text();
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
        flags,
        |(depth, _)| *depth,
        |(_, c)| selectable(c),
        |(_, c)| c.to_string(),
    )?
    .ok_or(CliError::Cancellation)?;
    eprintln!();
    Ok((*channel).clone())
}

/// Resolve a channel by ID, or prompt for one in the given server, or in a
/// server chosen at a prompt. IDs are on `host`, or the home host if it is
/// not given. Prompts offer only text channels when `text_only` is set.
/// `flags` name the arguments that would provide the channel instead.
pub async fn get_channel_selection_with_inputs(
    ctx: &mut CliContext<'_>,
    channel_id: Option<ServerChannelId>,
    server_id: Option<ServerId>,
    host: Option<&str>,
    text_only: bool,
    flags: &str,
) -> Result<ServerChannel, CliError> {
    let host = ctx.resource_host(host)?;
    match (channel_id, server_id) {
        (Some(channel_id), _) => {
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let channel = requests::channels::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                &ServerChannelRef::new(host, channel_id),
            )
            .await?;
            Ok(channel)
        }
        (None, Some(server_id)) => {
            get_channel_selection(
                ctx,
                &ServerRef::new(host, server_id),
                text_only,
                flags,
            )
            .await
        }
        (None, None) => {
            let server = get_server_selection(
                ctx,
                ServerSelectionType::MemberOnly,
                flags,
            )
            .await?;
            get_channel_selection(ctx, &server.to_ref(), text_only, flags).await
        }
    }
}
