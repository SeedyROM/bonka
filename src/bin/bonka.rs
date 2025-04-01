use std::process::exit;

use cli::{Command, RunCommand};
use color_eyre::eyre::Report;

use bonka::{cli, log, server};

#[tokio::main]
async fn main() -> Result<(), Report> {
    log::setup()?;
    let args = cli::parse_args();

    match args.command {
        Some(Command::Run(RunCommand { host, port })) => {
            log::info!("Starting bonka server on {}:{}", host, port);
            server::run(host, port).await
        }
        None => {
            eprintln!("No command provided. Use --help for more information.");
            exit(1);
        }
    }
}
