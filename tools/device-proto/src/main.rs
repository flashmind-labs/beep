//! Regenerate the checked-in device clients with `cargo run -p beep-device-proto`.

use std::{error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let node = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/node");
    let protos = node.join("proto");
    let output = node.join("src/run/device/proto");
    fs::create_dir_all(&output)?;

    let descriptors = protox::compile(
        [
            protos.join("emulator_controller.proto"),
            protos.join("idb.proto"),
        ],
        [&protos],
    )?;
    let mut config = tonic_prost_build::Config::new();
    config.disable_comments(["."]);

    tonic_prost_build::configure()
        .build_server(false)
        .build_transport(false)
        .disable_comments(["."])
        .out_dir(output)
        .compile_fds_with_config(descriptors, config)?;

    Ok(())
}
