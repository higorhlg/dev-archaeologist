fn main() {
    embed_cli_info_plist();
    tauri_build::build()
}

/// Gives the `dev-arch` binary an Info.plist with the Calendar usage text, so macOS can ask
/// for Calendar access when it runs outside the app bundle (terminal, sync agent).
fn embed_cli_info_plist() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let plist = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("cli-Info.plist");
    println!("cargo:rerun-if-changed={}", plist.display());
    println!("cargo:rustc-link-arg-bin=dev-arch=-Wl,-sectcreate,__TEXT,__info_plist,{}", plist.display());
}
