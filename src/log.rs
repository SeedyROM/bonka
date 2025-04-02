use color_eyre::Report;
pub use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

pub fn setup() -> Result<(), Report> {
    if cfg!(debug_assertions) {
        color_eyre::install()?;
    }

    let default_log_level = if cfg!(debug_assertions) {
        "info"
    } else {
        "error"
    };
    unsafe { std::env::set_var("RUST_LOG", default_log_level) };

    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}
