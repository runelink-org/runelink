use crossterm::{
    cursor::{Hide, MoveToColumn, MoveUp, Show},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::Print,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use runelink_client::requests;
use runelink_types::{Channel, Server};
use std::collections::HashSet;
use std::io::Write;
use uuid::Uuid;

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
    if items.is_empty() {
        println!("(no items to select)");
        return Ok(None);
    }
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    execute!(
        stdout,
        Hide,
        MoveToColumn(0),
        Print(format!("{}\n", prompt))
    )?;

    for (i, item) in items.iter().enumerate() {
        let prefix = if i == 0 { "> " } else { "  " };
        execute!(
            stdout,
            MoveToColumn(0),
            Print(format!("{}{}\n", prefix, display(item)))
        )?;
    }
    stdout.flush()?;

    let mut selected = 0;
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
                    selected = if selected == 0 { 0 } else { selected - 1 };
                }
                // Down or 'j'
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                    selected = if selected == items.len() - 1 {
                        items.len() - 1
                    } else {
                        selected + 1
                    };
                }
                (KeyCode::Char('g'), KeyModifiers::NONE)
                | (KeyCode::Home, _) => {
                    selected = 0;
                }
                (KeyCode::Char('g'), KeyModifiers::SHIFT)
                | (KeyCode::Char('G'), _)
                | (KeyCode::End, _) => {
                    selected = items.len() - 1;
                }
                _ => {}
            }

            // Redraw the list in place
            execute!(stdout, MoveUp(items.len() as u16))?;
            for (i, item) in items.iter().enumerate() {
                let prefix = if i == selected { "> " } else { "  " };
                execute!(
                    stdout,
                    Clear(ClearType::CurrentLine),
                    MoveToColumn(0),
                    Print(format!("{}{}\n", prefix, display(item)))
                )?;
            }
            stdout.flush()?;
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
    ctx: &CliContext<'_>,
    selection_type: ServerSelectionType<'_>,
) -> Result<Server, CliError> {
    let api_url = ctx.home_api_url()?;
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
            let member_server_ids = member_servers
                .iter()
                .map(|s| s.id)
                .collect::<HashSet<Uuid>>();
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
    pub server_id: Uuid,
    pub channel_id: Uuid,
}

pub async fn get_channel_selection(
    ctx: &mut CliContext<'_>,
    server_id: Uuid,
    server_host: &str,
) -> Result<ChannelSelection, CliError> {
    let api_url = ctx.home_api_url()?;
    let access_token = ctx.get_access_token().await?;
    let channels = requests::channels::fetch_by_server(
        ctx.client,
        &api_url,
        &access_token,
        server_id,
        Some(server_host),
    )
    .await?;
    if channels.is_empty() {
        return Err(CliError::NoActionPossible(
            "No channels available.\n\
                For more information, try `rune channel --help`."
                .into(),
        ));
    }
    let channel =
        select_inline(&channels, "Select channel", Channel::to_string)?
            .ok_or(CliError::Cancellation)?;
    println!();
    Ok(ChannelSelection {
        host: server_host.to_string(),
        server_id,
        channel_id: channel.id,
    })
}

pub async fn get_channel_selection_with_inputs(
    ctx: &mut CliContext<'_>,
    channel_id: Option<Uuid>,
    server_id: Option<Uuid>,
    host: Option<&str>,
) -> Result<ChannelSelection, CliError> {
    let host = match host {
        Some(host) => host.to_string(),
        None => ctx.home_host()?.to_string(),
    };
    match (channel_id, server_id) {
        (Some(channel_id), Some(server_id)) => Ok(ChannelSelection {
            host,
            server_id,
            channel_id,
        }),
        (Some(_channel_id), None) => Err(CliError::MissingContext(
            "Server ID must be passed with channel ID.".into(),
        )),
        (None, Some(server_id)) => {
            get_channel_selection(ctx, server_id, host.as_str()).await
        }
        (None, None) => {
            let server =
                get_server_selection(&*ctx, ServerSelectionType::MemberOnly)
                    .await?;
            get_channel_selection(ctx, server.id, server.host.as_str()).await
        }
    }
}
