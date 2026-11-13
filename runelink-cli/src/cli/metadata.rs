use runelink_types::metadata::{Metadata, validate_metadata};

use crate::error::CliError;

/// Metadata on a new resource. No flag means no metadata.
#[derive(clap::Args, Debug)]
pub struct MetadataArgs {
    /// Metadata as a JSON object, or null for none
    #[clap(long, value_name = "JSON", value_parser = parse_metadata)]
    pub metadata: Option<String>,
}

impl MetadataArgs {
    pub fn value(&self) -> Result<Option<Metadata>, CliError> {
        self.metadata
            .as_deref()
            .map(serde_json::from_str::<Option<Metadata>>)
            .transpose()
            .map(Option::flatten)
            .map_err(|e| CliError::InvalidArgument(e.to_string()))
    }
}

#[derive(clap::Args, Debug)]
pub struct MetadataEditArgs {
    #[clap(flatten)]
    pub value: MetadataArgs,
    /// Remove existing metadata
    #[clap(long, conflicts_with = "metadata")]
    pub clear_metadata: bool,
}

impl MetadataEditArgs {
    pub fn patch(&self) -> Result<Option<Option<Metadata>>, CliError> {
        if self.clear_metadata {
            Ok(Some(None))
        } else if self.value.metadata.is_some() {
            self.value.value().map(Some)
        } else {
            Ok(None)
        }
    }
}

fn parse_metadata(input: &str) -> Result<String, String> {
    let metadata: Option<Metadata> =
        serde_json::from_str(input).map_err(|e| e.to_string())?;
    if let Some(metadata) = metadata {
        validate_metadata(&metadata)?;
    }
    Ok(input.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        Cli, Commands, channels::ChannelCommands, messages::MessageCommands,
    };
    use clap::Parser;

    const ID: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn message_metadata_edits_do_not_require_a_body() {
        for flags in [
            vec![],
            vec!["--metadata", "null"],
            vec!["--clear-metadata"],
            vec!["--metadata", "{\"kind\":\"result\"}"],
        ] {
            let args =
                [vec!["rune", "message", "edit", "--message-id", ID], flags]
                    .concat();
            let cli = Cli::try_parse_from(args).unwrap();
            let Commands::Message(args) = cli.command else {
                panic!()
            };
            let MessageCommands::Edit(args) = args.command else {
                panic!()
            };
            assert!(args.body.is_none());
            assert_eq!(
                args.metadata.patch().unwrap().is_some(),
                args.metadata.clear_metadata
                    || args.metadata.value.metadata.is_some()
            );
        }
    }

    #[test]
    fn channel_metadata_edits_do_not_require_details() {
        let cli = Cli::try_parse_from([
            "rune",
            "channel",
            "edit",
            "--channel-id",
            ID,
            "--clear-metadata",
        ])
        .unwrap();
        let Commands::Channel(args) = cli.command else {
            panic!()
        };
        let ChannelCommands::Edit(args) = args.command else {
            panic!()
        };
        assert!(args.title.is_none());
        assert!(args.description.is_none());
        assert_eq!(args.metadata.patch().unwrap(), Some(None));
    }

    #[test]
    fn invalid_metadata_and_conflicting_flags_fail_before_authentication() {
        for command in ["message", "channel"] {
            let id_flag = if command == "message" {
                "--message-id"
            } else {
                "--channel-id"
            };
            for value in ["[]", "true", "42", "invalid"] {
                assert!(
                    Cli::try_parse_from([
                        "rune",
                        command,
                        "edit",
                        id_flag,
                        ID,
                        "--metadata",
                        value
                    ])
                    .is_err()
                );
            }
            assert!(
                Cli::try_parse_from([
                    "rune",
                    command,
                    "edit",
                    id_flag,
                    ID,
                    "--metadata",
                    "{}",
                    "--clear-metadata"
                ])
                .is_err()
            );
        }
        assert!(
            parse_metadata(&format!(
                "{{\"value\":\"{}\"}}",
                "x".repeat(64 * 1024)
            ))
            .is_err()
        );
    }
}
