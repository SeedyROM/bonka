fn main() {
    built::write_built_file().expect("Failed to acquire build-time information");

    // Compile protocol buffers
    println!("cargo:rerun-if-changed=../protocol/bonka.proto");
    prost_build::compile_protos(&["../protocol/bonka.proto"], &["../protocol"])
        .expect("Failed to compile protobuf definitions");
}
