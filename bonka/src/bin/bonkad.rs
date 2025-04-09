use std::process::exit;

use cli::{Command, RunCommand};
use color_eyre::eyre::Report;

use bonka::{cli, log, server};

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    log::setup()?;
    let args = cli::parse_args();

    match args.command {
        Some(Command::Run(RunCommand { host, port })) => {
            print_logo();

            if let Err(err) = server::run(host, port).await {
                log::error!("Error running server: {}", err);
                exit(1);
            }
        }
        None => {
            eprintln!("No command provided. Use --help for more information.");
            exit(1);
        }
    }

    Ok(())
}

fn print_logo() {
    println!(
        r#"
    d8b                         d8b                 
     ?88                         ?88                 
      88b                         88b                
      888888b  d8888b   88bd88b   888  d88' d888b8b  
      88P `?8bd8P' ?88  88P' ?8b  888bd8P' d8P' ?88  
     d88,  d8888b  d88 d88   88P d88888b   88b  ,88b 
    d888888P'`?8888P'd88'   88bd88' `?88b,`?88P'`88b    (v{} / {} / {})
    "#,
        built_info::PKG_VERSION,
        built_info::PROFILE,
        built_info::TARGET,
        // NOTE(SeedyROM): The git commit hash is incorrect apparently...
        // built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("unknown"),
    );
}
