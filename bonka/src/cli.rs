use clap::Parser;

pub const DEFAULT_HOST: &str = "[::1]";
pub const DEFAULT_PORT: u16 = 8379;

#[derive(Debug, Parser)]
#[clap(
    name = "bonka",
    about = "A general purpose kv store",
    long_about = "Bonka is a general purpose key-value store that can be used for various applications. It is designed to be fast, reliable, and easy to use.",
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS")
)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Parser)]
#[clap(name = "Command", about = "Subcommands for sccache")]
pub enum Command {
    Run(RunCommand),
}

#[derive(Debug, Parser)]
#[clap(name = "run", about = "Run the bonka server")]
pub struct RunCommand {
    #[clap(
        long,
        default_value = DEFAULT_HOST,
        env = "BONKA_HOST",
        help = "The host to bind to")
    ]
    pub host: String,
    #[clap(
        long,
        default_value_t = DEFAULT_PORT,
        env = "BONKA_PORT",
        help = "The port to bind to"
    )]
    pub port: u16,
}

pub fn parse_args() -> Args {
    Args::parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serial_test::serial;
    use std::env;

    #[test]
    #[serial]
    fn default_values() {
        // Clear any env vars that might interfere
        unsafe {
            env::remove_var("BONKA_HOST");
            env::remove_var("BONKA_PORT");
        }

        // Test with just the run command
        let args = Args::parse_from(["bonka", "run"]);

        // Verify we get a Run command
        assert!(matches!(args.command, Some(Command::Run(run_cmd)) if 
            run_cmd.host == DEFAULT_HOST && run_cmd.port == DEFAULT_PORT));
    }

    #[test]
    fn command_line_args() {
        // Test command-line arguments
        let args = Args::parse_from(["bonka", "run", "--host", "127.0.0.1", "--port", "9000"]);

        // Verify we get a Run command
        assert!(
            matches!(args.command, Some(Command::Run(run_cmd)) if run_cmd.host == "127.0.0.1" && run_cmd.port == 9000)
        );
    }

    #[test]
    #[serial]
    fn env_vars() {
        // Set environment variables
        unsafe {
            env::set_var("BONKA_HOST", "192.168.1.1");
            env::set_var("BONKA_PORT", "5000");
        }

        // Parse with no command line args (should use env vars)
        let args = Args::parse_from(["bonka", "run"]);

        // Verify we get a Run command
        assert!(
            matches!(args.command, Some(Command::Run(run_cmd)) if run_cmd.host == "192.168.1.1" && run_cmd.port == 5000)
        );

        // Clean up
        unsafe {
            env::remove_var("BONKA_HOST");
            env::remove_var("BONKA_PORT");
        }
    }

    #[test]
    #[serial]
    fn command_line_precedence() {
        // Set environment variables
        unsafe {
            env::set_var("BONKA_HOST", "192.168.1.1");
            env::set_var("BONKA_PORT", "5000");
        }

        // Command line args should take precedence
        let args = Args::parse_from(["bonka", "run", "--host", "127.0.0.1", "--port", "9000"]);

        // Verify we get a Run command
        assert!(
            matches!(args.command, Some(Command::Run(run_cmd)) if run_cmd.host == "127.0.0.1" && run_cmd.port == 9000)
        );

        // Clean up
        unsafe {
            env::remove_var("BONKA_HOST");
            env::remove_var("BONKA_PORT");
        }
    }

    #[test]
    #[serial]
    fn no_command() {
        // Test with no subcommand
        let args = Args::parse_from(["bonka"]);
        assert!(args.command.is_none());
    }

    #[test]
    #[serial]
    fn run_command_default() {
        let args = Args::parse_from(["bonka", "run"]);
        assert!(matches!(args.command, Some(Command::Run(run_cmd)) if 
            run_cmd.host == DEFAULT_HOST && run_cmd.port == DEFAULT_PORT));
    }
}
