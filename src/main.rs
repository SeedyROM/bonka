use std::process::exit;

use cli::{Command, RunCommand};
use color_eyre::eyre::Report;
use log::info;

pub mod cli;
pub mod constants;
pub mod kv;
pub mod log;
pub mod protocol;
pub mod server;
pub mod session;

#[tokio::main]
async fn main() -> Result<(), Report> {
    log::setup()?;
    let args = cli::parse_args();

    match args.command {
        Some(Command::Run(RunCommand { host, port })) => {
            info!("Starting bonka server on {}:{}", host, port);
            server::run(host, port).await
        }
        None => {
            eprintln!("No command provided. Use --help for more information.");
            exit(1);
        }
    }
}
