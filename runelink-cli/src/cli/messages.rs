use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use runelink_client::requests;
use runelink_types::{
    channel::{ServerChannelContent, ServerChannelId, TextChannelId},
    message::{Message, MessageId, NewMessage},
    server::ServerId,
};

use crate::{
    error::CliError,
    util::{list_label, parse_optional_host_input},
};

use super::{
    context::CliContext, input::unwrap_or_prompt,
    select::get_channel_selection_with_inputs,
};

#[derive(clap::Args, Debug)]
pub struct MessageArgs {
    #[clap(subcommand)]
    pub command: MessageCommands,
}

#[derive(clap::Subcommand, Debug)]
pub enum MessageCommands {
    /// Edit an existing message
    Edit(MessageEditArgs),
    /// List all messages
    List(MessageListArgs),
    /// Get a message by ID
    Get(MessageGetArgs),
    /// Send a message
    Send(MessageSendArgs),
    /// Delete a message
    Delete(MessageDeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct MessageListArgs {
    /// Show the full message ID
    #[clap(long, conflicts_with = "hide_ids")]
    pub show_ids: bool,
    /// Hide IDs (the default)
    #[clap(long)]
    pub hide_ids: bool,
    #[clap(flatten)]
    pub channel: TextChannelArgs,
}

#[derive(clap::Args, Debug)]
pub struct MessageGetArgs {
    /// The ID of the message to fetch
    #[clap(long)]
    pub message_id: MessageId,
    /// The host of the server the message is in
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct MessageSendArgs {
    /// The message to reply to, within the same text channel
    #[clap(long)]
    pub reply_to: Option<MessageId>,
    /// The body of the message
    #[clap(long)]
    pub body: Option<String>,
    #[clap(flatten)]
    pub channel: TextChannelArgs,
}

/// The text channel to list or send messages in. Prompts for a channel when
/// no ID is given.
#[derive(clap::Args, Debug)]
pub struct TextChannelArgs {
    /// The server to select a channel from
    #[clap(long, conflicts_with_all = ["channel_id", "text_channel_id"])]
    pub server_id: Option<ServerId>,
    /// The ID of the channel
    #[clap(long, conflicts_with = "text_channel_id")]
    pub channel_id: Option<ServerChannelId>,
    /// The ID of the underlying text channel
    #[clap(long)]
    pub text_channel_id: Option<TextChannelId>,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct MessageDeleteArgs {
    /// The ID of the message to delete
    #[clap(long)]
    pub message_id: MessageId,
    /// The host of the server
    #[clap(long)]
    pub host: Option<String>,
}

pub async fn handle_message_commands(
    ctx: &mut CliContext<'_>,
    message_args: &MessageArgs,
) -> Result<(), CliError> {
    match &message_args.command {
        MessageCommands::Edit(edit_args) => {
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                edit_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let original = requests::messages::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                edit_args.message_id,
                target_host.as_deref(),
            )
            .await?;
            let update = runelink_types::message::MessageUpdate {
                body: edit_args.body.clone(),
                expected_updated_at: original.updated_at,
            };
            let resource = requests::messages::update(
                ctx.client,
                &api_url,
                &access_token,
                edit_args.message_id,
                &update,
                target_host.as_deref(),
            )
            .await?;
            println!("Updated message: {resource}");
        }

        MessageCommands::List(list_args) => {
            let show_ids = list_args.show_ids && !list_args.hide_ids;
            ctx.account.ok_or(CliError::MissingAccount)?;
            let (text_channel_id, target_host) =
                resolve_text_channel(ctx, &list_args.channel).await?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let messages = requests::messages::fetch_by_text_channel(
                ctx.client,
                &api_url,
                &access_token,
                text_channel_id,
                target_host.as_deref(),
            )
            .await?;
            let messages_by_id: HashMap<_, _> = messages
                .iter()
                .map(|message| (message.id, message))
                .collect();
            let width = crossterm::terminal::size()
                .ok()
                .map(|(columns, _)| usize::from(columns))
                .filter(|columns| *columns > 0)
                .unwrap_or(80);
            for message in messages.iter().rev() {
                let Some(body) = message.content.body() else {
                    continue;
                };
                if let Some(reply_to) = message.reply_to {
                    println!(
                        "{}",
                        reply_preview(
                            messages_by_id.get(&reply_to).copied(),
                            width,
                            show_ids,
                        )
                    );
                }
                println!("{}: {body}", author_label(message, show_ids));
            }
        }

        MessageCommands::Get(get_args) => {
            ctx.account.ok_or(CliError::MissingAccount)?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                get_args.host.as_deref(),
                ctx.strict_input,
            )?;
            let message = requests::messages::fetch_by_id(
                ctx.client,
                &api_url,
                &access_token,
                get_args.message_id,
                target_host.as_deref(),
            )
            .await?;
            println!("{message}");
        }

        MessageCommands::Send(send_args) => {
            let account = ctx.account.ok_or(CliError::MissingAccount)?;
            let (text_channel_id, target_host) =
                resolve_text_channel(ctx, &send_args.channel).await?;
            let body = unwrap_or_prompt(send_args.body.clone(), "Message")?;
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let new_message = NewMessage {
                author: account.user_ref.clone(),
                body,
                reply_to: send_args.reply_to,
            };
            let message = requests::messages::create(
                ctx.client,
                &api_url,
                &access_token,
                text_channel_id,
                &new_message,
                target_host.as_deref(),
            )
            .await?;
            println!("Sent message: {message}");
        }

        MessageCommands::Delete(delete_args) => {
            // TODO: Interactive message selection
            let api_url = ctx.home_api_url().await?;
            let access_token = ctx.get_access_token().await?;
            let target_host = parse_optional_host_input(
                delete_args.host.as_deref(),
                ctx.strict_input,
            )?;
            requests::messages::delete(
                ctx.client,
                &api_url,
                &access_token,
                delete_args.message_id,
                target_host.as_deref(),
            )
            .await?;
            println!("Deleted message: {}", delete_args.message_id);
        }
    };
    Ok(())
}

// Resolve the text channel and its target host, which is `None` for the home
// host, prompting for whatever is missing
async fn resolve_text_channel(
    ctx: &mut CliContext<'_>,
    args: &TextChannelArgs,
) -> Result<(TextChannelId, Option<String>), CliError> {
    let home_host = ctx.home_host()?.to_string();
    let target_host = |host: String| (host != home_host).then_some(host);
    let host =
        parse_optional_host_input(args.host.as_deref(), ctx.strict_input)?;
    if let Some(text_channel_id) = args.text_channel_id {
        return Ok((text_channel_id, host.and_then(target_host)));
    }
    let selection = get_channel_selection_with_inputs(
        ctx,
        args.channel_id,
        args.server_id,
        host.as_deref(),
        true,
    )
    .await?;
    let ServerChannelContent::Text { text_channel_id } =
        selection.channel.content
    else {
        return Err(CliError::InvalidArgument(format!(
            "{} is not a text channel.",
            selection.channel.verbose()
        )));
    };
    Ok((text_channel_id, target_host(selection.host)))
}

fn author_label(message: &Message, show_ids: bool) -> String {
    let name = message
        .author
        .as_ref()
        .map(|author| author.name.as_str())
        .unwrap_or("anon");
    list_label(name, message.id, show_ids)
}

fn reply_preview(
    target: Option<&Message>,
    columns: usize,
    show_ids: bool,
) -> String {
    let (prefix, preview) = match target {
        Some(message) => match message.content.body() {
            Some(body) => {
                let (first_line, more_lines) = body
                    .split_once(['\r', '\n'])
                    .map_or((body, false), |(first, _)| (first, true));
                // Keep tabs and other controls from changing the preview layout.
                let first_line = first_line.replace(char::is_control, " ");
                (
                    format!("  ┌─ {}: ", author_label(message, show_ids)),
                    format!(
                        "{first_line}{}",
                        if more_lines { " ..." } else { "" }
                    ),
                )
            }
            None => ("  ┌─ ".into(), "Original message was deleted".into()),
        },
        None => ("  ┌─ ".into(), "Original message unavailable".into()),
    };
    let prefix_width = prefix.graphemes(true).map(preview_grapheme_width).sum();
    // Preserve the label and a complete ellipsis even in a narrower terminal.
    let text_columns = columns.saturating_sub(prefix_width).max(3);
    format!("{prefix}{}", truncate_preview(&preview, text_columns))
}

fn truncate_preview(preview: &str, columns: usize) -> String {
    if preview
        .graphemes(true)
        .map(preview_grapheme_width)
        .sum::<usize>()
        <= columns
    {
        return preview.into();
    }
    if columns < 3 {
        return ".".repeat(columns);
    }
    let mut result = String::new();
    let mut width = 0;
    for grapheme in preview.graphemes(true) {
        let next_width = preview_grapheme_width(grapheme);
        if width + next_width > columns - 3 {
            break;
        }
        result.push_str(grapheme);
        width += next_width;
    }
    format!("{}...", result.trim_end())
}

// Some terminals render joined emoji as separate glyphs. Reserve enough
// columns for either rendering, while still keeping the entire grapheme intact.
fn preview_grapheme_width(grapheme: &str) -> usize {
    let separate_width = grapheme
        .chars()
        .map(|character| character.width().unwrap_or(0))
        .sum();
    grapheme.width().max(separate_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runelink_types::{
        message::MessageContent,
        user::{User, UserRole},
    };
    use time::OffsetDateTime;

    fn original(body: &str) -> Message {
        Message {
            id: MessageId::new(),
            text_channel_id: TextChannelId::new(),
            author: Some(User {
                name: "mandar".into(),
                host: "localhost".into(),
                role: UserRole::User,
                created_at: OffsetDateTime::UNIX_EPOCH,
                updated_at: OffsetDateTime::UNIX_EPOCH,
                synced_at: None,
            }),
            reply_to: None,
            content: MessageContent::Active {
                body: body.into(),
                deleted_at: (),
            },
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            edited_at: None,
        }
    }

    #[test]
    fn preview_includes_author_and_only_the_first_line() {
        assert_eq!(
            reply_preview(
                Some(&original("Does 5 PM work for you?")),
                80,
                false
            ),
            "  ┌─ mandar: Does 5 PM work for you?"
        );
        for body in [
            "First line\nSecond line",
            "First line\r\nSecond line",
            "First line\n",
        ] {
            assert_eq!(
                reply_preview(Some(&original(body)), 80, false),
                "  ┌─ mandar: First line ..."
            );
        }
    }

    #[test]
    fn width_limit_includes_indent_author_and_multiline_marker() {
        assert_eq!(
            reply_preview(Some(&original("abcdefghijklmnop")), 22, false),
            "  ┌─ mandar: abcdef..."
        );
        assert_eq!(
            reply_preview(Some(&original("First line\nSecond")), 26, false),
            "  ┌─ mandar: First line..."
        );
    }

    #[test]
    fn full_ids_are_shared_by_message_labels_and_preview_widths() {
        let mut message = original("abcdefghijklmnop");
        let id = "7f3b0d26-0000-4000-8000-000000000001";
        message.id = id.parse().unwrap();
        assert_eq!(author_label(&message, true), format!("mandar [{id}]"));
        assert_eq!(
            reply_preview(Some(&message), 80, true),
            format!("  ┌─ mandar [{id}]: abcdefghijklmnop")
        );
        assert_eq!(
            reply_preview(Some(&message), 61, true),
            format!("  ┌─ mandar [{id}]: abcdef...")
        );
        for columns in [0, 1, 2, 8, 16] {
            assert_eq!(
                reply_preview(Some(&message), columns, true),
                format!("  ┌─ mandar [{id}]: ...")
            );
            assert_eq!(
                reply_preview(Some(&message), columns, false),
                "  ┌─ mandar: ..."
            );
        }
    }

    #[test]
    fn unicode_previews_preserve_graphemes_and_fit_unjoined_emoji() {
        // Keep graphemes intact and allow eight columns for four unjoined emoji.
        // Even the fast path must allow for terminals that do not join them.
        for (input, columns, expected) in [
            ("a界e\u{301}👨‍👩‍👧‍👦bcdef", 8, "a界e\u{301}..."),
            ("👨‍👩‍👧‍👦123456789", 12, "👨‍👩‍👧‍👦1..."),
            ("👨‍👩‍👧‍👦abcd", 8, "..."),
        ] {
            assert_eq!(truncate_preview(input, columns), expected);
        }
        for columns in 0..3 {
            assert_eq!(truncate_preview("hello", columns), ".".repeat(columns));
        }
        let preview = reply_preview(
            Some(&original(
                "5. Unicode width: 界界界 cafe\u{301} 👨‍👩‍👧‍👦. Wide characters and combined emoji should stay intact when truncated.",
            )),
            80,
            false,
        );
        let separate_columns: usize =
            preview.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert!(separate_columns <= 80);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn deleted_or_missing_targets_do_not_expose_text() {
        let mut message = original("erased text");
        message.content = MessageContent::Deleted {
            body: (),
            deleted_at: OffsetDateTime::UNIX_EPOCH,
        };
        assert_eq!(
            reply_preview(Some(&message), 80, false),
            "  ┌─ Original message was deleted"
        );
        assert_eq!(
            reply_preview(None, 80, false),
            "  ┌─ Original message unavailable"
        );
    }
}

#[derive(clap::Args, Debug)]
pub struct MessageEditArgs {
    #[clap(long)]
    pub message_id: MessageId,
    #[clap(long)]
    pub host: Option<String>,
    /// Replacement message text
    #[clap(long)]
    pub body: String,
}
