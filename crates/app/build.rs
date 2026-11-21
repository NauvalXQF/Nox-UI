include!("icon_res.rs");

fn main() {
    println!("cargo:rerun-if-changed=icon_res.rs");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("abra.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    embed_icon();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
