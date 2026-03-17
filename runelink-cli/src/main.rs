use clap::Parser;
use cli::handle_cli;
use reqwest::Client;
use runelink_client::{requests, util::get_api_url};
use std::process::ExitCode;
use storage::AppConfig;
use storage_auth::AuthCache;

use crate::{cli::Cli, error::CliError};

mod cli;
mod error;
mod storage;
mod storage_auth;
mod util;

#[allow(dead_code)]
async fn test_connectivities(client: &Client, hosts: Vec<&str>) {
    println!("Hosts:");
    for host in hosts {
        let api_url = get_api_url(host);
        match requests::ping(client, &api_url).await {
            Ok(_) => println!("{} (ready)", host),
            Err(_) => println!("{} (down)", host),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    async fn run_app() -> Result<(), CliError> {
        let mut config = AppConfig::load()?;
        let mut auth_cache = AuthCache::load()?;
        let cli = Cli::parse();
        let client = Client::new();
        handle_cli(&client, &cli, &mut config, &mut auth_cache).await?;
        auth_cache.save()?;
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
