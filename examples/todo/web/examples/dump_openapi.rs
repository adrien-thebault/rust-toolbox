//! Print the gateway's OpenAPI spec on stdout.
//!
//! CI redirects this into the committed `openapi.json` and fails on a diff, so
//! a route whose schema changed without the spec being regenerated is a build
//! failure naming exactly what moved. `examples/todo/openapi.sh` is the
//! same thing by hand.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "{}",
        toolbox::web::openapi::serialize_openapi(&todo_web::routes::openapi())?
    );
    Ok(())
}
