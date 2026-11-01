//! Compiles the interface's translations (assets/i18n/*.po) and embeds the
//! icon in the Windows executable (assets/cordfast.rc). A machine without a
//! resource compiler still builds, without the icon.

fn main() {
    fastframe_i18n::build::compile_catalogs("assets/i18n");
    println!("cargo:rerun-if-changed=assets/cordfast.rc");
    println!("cargo:rerun-if-changed=assets/cordfast.ico");
    #[cfg(windows)]
    if let Err(error) = embed_resource::compile("assets/cordfast.rc", embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=icon not embedded: {error}");
    }
}
