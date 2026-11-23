fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/center/spaces.m");
        cc::Build::new()
            .file("src/center/spaces.m")
            .flag("-fobjc-arc")
            .compile("window_spaces");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }

    tauri_build::build();
}
