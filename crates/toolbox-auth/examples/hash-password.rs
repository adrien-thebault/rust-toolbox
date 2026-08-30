//! Print an argon2 hash for a password, for seeding a user store.
//!
//! ```sh
//! ./scripts/hash-password.sh 'correct horse battery staple'
//! ./scripts/hash-password.sh          # prompts, so it stays out of your shell history
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let password = if let Some(given) = std::env::args().nth(1) {
        given
    } else {
        eprint!("password: ");
        rpassword::read_password()?
    };

    if password.is_empty() {
        eprintln!("refusing to hash an empty password");
        std::process::exit(1);
    }
    println!("{}", toolbox_auth::hash_password(&password)?);
    Ok(())
}
