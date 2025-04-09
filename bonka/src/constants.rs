use once_cell::sync::Lazy;
use rand::{TryRngCore, rngs::OsRng};

/// Default application secret used for session ID generation.
/// This is generated randomly at runtime if not provided via the environment variable `BONKA_APP_SECRET`.
pub static DEFAULT_APP_SECRET: Lazy<[u8; 32]> = Lazy::new(|| {
    let mut secret = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut secret)
        .expect("Failed to fill DEFAULT_APP_SECRET with random bytes");
    secret
});
