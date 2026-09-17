//! Compile the toolbox's own protos.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        .compile_protos(&["proto/toolbox/v1/pagination.proto"], &["proto"])?;
    Ok(())
}
