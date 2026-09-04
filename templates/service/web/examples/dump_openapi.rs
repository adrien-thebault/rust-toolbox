//! Print the gateway's OpenAPI spec on stdout.
//!
//! CI redirects this into the committed `web/openapi.json` and fails on a diff,
//! so a route whose schema changed without the spec being regenerated is a
//! build failure naming exactly what moved. `./openapi.sh` is the same thing by
//! hand.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        toolbox_web::openapi::dump_openapi(&{{crate_name}}_web::routes::openapi())?
    );
    Ok(())
}
