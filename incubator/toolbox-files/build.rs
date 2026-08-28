//! Compile this crate's own proto.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::configure()
        // `bytes` fields become `Bytes` rather than `Vec<u8>`, so a chunk is
        // not copied out of the wire buffer on the way through.
        .bytes(".")
        .compile_protos(&["proto/toolbox/v1/file.proto"], &["proto"])?;
    Ok(())
}
