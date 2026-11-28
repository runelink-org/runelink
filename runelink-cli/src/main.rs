use clap::Parser;
use cli::handle_cli;
use std::process::ExitCode;
use storage::AppConfig;
use storage_auth::AuthCache;

use crate::{cli::Cli, error::CliError};

mod cli;
mod error;
mod storage;
mod storage_auth;
mod transport;
mod util;

#[tokio::main]
async fn main() -> ExitCode {
    async fn run_app() -> Result<(), CliError> {
        let mut config = AppConfig::load()?;
        let mut auth_cache = AuthCache::load()?;
        let cli = Cli::parse();
        let client = transport::build_client()?;
        handle_cli(&client, &cli, &mut config, &mut auth_cache).await?;
        // Changes to the auth cache are saved as they are made. Saving it
        // here would overwrite tokens that other commands refreshed meanwhile.
        config.save()?;
        Ok(())
    }

    match run_app().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(cli_error) => {
            cli_error.report_for_cli();
            cli_error.into()
        }
    }
}
