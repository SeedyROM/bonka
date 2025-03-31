use std::process::exit;

use cli::Command;
use color_eyre::eyre::Report;
use log::info;

pub mod cli;
pub mod constants;
pub mod log;
pub mod protocol;
pub mod server;
pub mod session;

fn main() -> Result<(), Report> {
    log::setup()?;
    let args = cli::parse_args();

    match args.command {
        Some(Command::Run(run_args)) => {
            info!(
                "Starting bonka server on {}:{}",
                run_args.host, run_args.port
            );
        }
        None => {
            eprintln!("No command provided. Use --help for more information.");
            exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
#[cfg(not(coverage))]
mod main {
    mod tests {
        use assert_cmd::Command;

        #[test]
        fn no_command() {
            let mut cmd = Command::cargo_bin("bonka").unwrap();
            cmd.assert()
                .failure()
                .code(1)
                .stderr(predicates::str::contains(
                    "No command provided. Use --help for more information.",
                ));
        }

        #[test]
        fn invalid_command() {
            let mut cmd = Command::cargo_bin("bonka").unwrap();
            cmd.args(&["invalid"])
                .assert()
                .failure()
                .code(2)
                .stderr(predicates::str::contains(
                    "unrecognized subcommand 'invalid'",
                ));
        }

        #[test]
        fn run_no_values() {
            let mut cmd = Command::cargo_bin("bonka").unwrap();
            cmd.args(&["run"])
                .assert()
                .success()
                .stdout(predicates::str::contains(
                    "Starting bonka server on ::0:8379",
                ));
        }

        #[test]
        fn run_host_specified() {
            let mut cmd = Command::cargo_bin("bonka").unwrap();
            cmd.args(&["run", "--host", "0.0.0.0"])
                .assert()
                .success()
                .stdout(predicates::str::contains(
                    "Starting bonka server on 0.0.0.0:8379",
                ));
        }

        #[test]
        fn run_port_specified() {
            let mut cmd = Command::cargo_bin("bonka").unwrap();
            cmd.args(&["run", "--port", "9000"])
                .assert()
                .success()
                .stdout(predicates::str::contains(
                    "Starting bonka server on ::0:9000",
                ));
        }
    }
}
