fn main() {
    tauri_plugin::Builder::new(&["scan"])
        .android_path("android")
        .build();
}
