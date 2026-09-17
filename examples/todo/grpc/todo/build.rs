//! Compile this example's protos, with the toolbox's on the include path.

use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        // Point the toolbox's messages at the facade's generated types
        // generated, instead of generating a second copy of them here. Without
        // this, prost emits `super::super::toolbox::v1::PageRequest`, which
        // refers to a module this crate does not have.
        .extern_path(".toolbox.v1", "::toolbox::grpc::pagination::proto")
        .file_descriptor_set_path(PathBuf::from(env::var("OUT_DIR")?).join("todo_descriptor.bin"))
        .compile_protos(
            &["proto/todo/v1/todo.proto"],
            // The second entry is what lets the proto `import
            // "toolbox/v1/pagination.proto"` rather than copy it.
            &["proto", toolbox::grpc::PROTO_INCLUDE],
        )?;
    Ok(())
}
