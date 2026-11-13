//! Compiles the gRPC clients for the Android emulator and, on macOS, for
//! idb_companion, from the protos in `proto/`. Only a node that hosts
//! devices (the `host` feature) needs them.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "host")]
    {
        let mut protos = vec!["proto/emulator_controller.proto"];

        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
            protos.push("proto/idb.proto");
        }

        for proto in &protos {
            println!("cargo:rerun-if-changed={proto}");
        }

        let descriptors = protox::compile(&protos, ["proto"])?;
        tonic_prost_build::configure()
            .build_server(false)
            .build_transport(false)
            .compile_fds(descriptors)?;
    }

    Ok(())
}
