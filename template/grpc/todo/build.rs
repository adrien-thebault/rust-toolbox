//! Compile this service's protos, with the toolbox's on the include path.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        // Required. Without it prost emits `super::super::toolbox::v1::PageRequest`
        // for the imported messages, which is a module this crate does not have.
        .extern_path(".toolbox.v1", "::toolbox_grpc::pagination::proto")
        .file_descriptor_set_path(
            std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("todo_descriptor.bin"),
        )
        .compile_protos(
            &["proto/todo/v1/todo.proto"],
            &["proto", toolbox_grpc::PROTO_INCLUDE],
        )?;
    Ok(())
}
