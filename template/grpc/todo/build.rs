//! Compile this service's protos, with the toolbox's on the include path.

use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        // Required. Without it prost emits `super::super::toolbox::v1::PageRequest`
        // for the imported messages, which is a module this crate does not have.
        .extern_path(".toolbox.v1", "::toolbox::grpc::pagination::proto")
        .file_descriptor_set_path(PathBuf::from(env::var("OUT_DIR")?).join("todo_descriptor.bin"))
        .compile_protos(
            &["proto/todo/v1/todo.proto"],
            &["proto", toolbox::grpc::PROTO_INCLUDE],
        )?;
    Ok(())
}
