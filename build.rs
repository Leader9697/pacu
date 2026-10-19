#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=res/pacu.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("pacu.ico"), icon::ico_bytes(&[16, 24, 32, 48, 64, 256])).unwrap();
    std::fs::copy("res/pacu.manifest", out.join("pacu.manifest")).unwrap();
    let rc = std::fs::read_to_string("res/pacu.rc").unwrap();
    std::fs::write(out.join("pacu.rc"), rc).unwrap();
    embed_resource::compile(out.join("pacu.rc"), embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
