fn main() {
    #[cfg(feature = "host")]
    compile_device_protos();
}

#[cfg(feature = "host")]
fn compile_device_protos() {
    use std::{env, path::PathBuf};

    let protos =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("proto");
    let emulator = protos.join("emulator_controller.proto");
    let idb = protos.join("idb.proto");
    println!("cargo:rerun-if-changed={}", emulator.display());
    println!("cargo:rerun-if-changed={}", idb.display());

    let descriptors =
        protox::compile([emulator, idb], [&protos]).expect("compiling device protobuf schemas");
    let mut config = tonic_prost_build::Config::new();
    config.disable_comments(["."]);

    tonic_prost_build::configure()
        .build_server(false)
        .build_transport(false)
        .disable_comments(["."])
        .compile_fds_with_config(descriptors, config)
        .expect("generating device gRPC clients");
}
