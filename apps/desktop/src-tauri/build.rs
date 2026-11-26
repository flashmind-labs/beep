use std::{path::PathBuf, process::Command};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/center/spaces.m");
        cc::Build::new()
            .file("src/center/spaces.m")
            .flag("-fobjc-arc")
            .compile("window_spaces");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");

        // rustc links macOS binaries with `-nodefaultlibs`, omitting Clang's
        // Darwin runtime (`libclang_rt.osx.a`) where `___isPlatformVersionAtLeast`
        // lives for Objective-C `@available(...)` checks in `ggml-metal`.
        let dir = macos_clang_runtime_dir();
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static:-bundle=clang_rt.osx");
    }

    tauri_build::build();
}

fn macos_clang_runtime_dir() -> PathBuf {
    for (program, args) in [
        ("xcrun", &["clang", "--print-runtime-dir"][..]),
        ("clang", &["--print-runtime-dir"][..]),
    ] {
        if let Some(dir) = command_path(program, args)
            && dir.join("libclang_rt.osx.a").is_file()
        {
            return dir;
        }
    }

    for (program, args) in [
        ("xcrun", &["clang", "--print-resource-dir"][..]),
        ("clang", &["--print-resource-dir"][..]),
    ] {
        if let Some(resource_dir) = command_path(program, args) {
            let dir = resource_dir.join("lib/darwin");
            if dir.join("libclang_rt.osx.a").is_file() {
                return dir;
            }
        }
    }

    panic!("could not locate libclang_rt.osx.a from Xcode or clang");
}

fn command_path(program: &str, args: &[&str]) -> Option<PathBuf> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}
