//! Print an Argon2 password hash for the seeded account.

use std::{env, error::Error, process};

fn main() -> Result<(), Box<dyn Error>> {
    let password = if let Some(given) = env::args().nth(1) {
        given
    } else {
        eprint!("password: ");
        rpassword::read_password()?
    };

    if password.is_empty() {
        eprintln!("refusing to hash an empty password");
        process::exit(1);
    }
    println!("{}", toolbox::auth::hash_password(&password)?);
    Ok(())
}
