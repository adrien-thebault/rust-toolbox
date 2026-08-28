//! Print the gateway's OpenAPI spec on stdout.
//!
//! CI redirects this into the committed `openapi.json` and fails on a diff, so
//! a route whose schema changed without the spec being regenerated is a build
//! failure naming exactly what moved. `./scripts/openapi.sh` is the same thing
//! by hand.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        toolbox_web::openapi::dump_openapi(&example_web::routes::openapi())?
    );
    Ok(())
}
