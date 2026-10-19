//! Preset aplikasi beban 3D.

/// Argumen setara preset Extreme di peluncur Heaven 4.0 (DirectX 11, kualitas Ultra, Tessellation
/// Extreme, anti-aliasing 8x, 1600x900), tetapi dalam jendela supaya Pacu Volt tetap terlihat, dan
/// tanpa suara. Heaven berputar terus sampai ditutup.
pub const HEAVEN_ARGS: &str = "-project_name Heaven -data_path ../ -engine_config ../data/heaven_4.0.cfg \
-system_script heaven/unigine.cpp -sound_app null -video_app direct3d11 -video_multisample 3 \
-video_fullscreen 0 -video_mode -1 -video_width 1600 -video_height 900 \
-extern_define \",RELEASE,QUALITY_ULTRA,TESSELLATION_EXTREME\"";

/// Mencari Heaven.exe di lokasi pemasangan bawaan.
pub fn detect_heaven() -> Option<(String, String)> {
    let roots = [std::env::var("ProgramFiles(x86)").ok(), std::env::var("ProgramFiles").ok(), Some("C:\\Program Files (x86)".into())];
    for r in roots.into_iter().flatten() {
        let p = std::path::Path::new(&r).join("Unigine").join("Heaven Benchmark 4.0").join("bin").join("Heaven.exe");
        if p.exists() {
            return Some((p.display().to_string(), HEAVEN_ARGS.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn heaven_args_keep_quoted_defines() {
        let a = crate::volt::rig::split_args(super::HEAVEN_ARGS);
        assert!(a.contains(&",RELEASE,QUALITY_ULTRA,TESSELLATION_EXTREME".to_string()));
        assert!(a.windows(2).any(|w| w[0] == "-video_width" && w[1] == "1600"));
        assert!(a.windows(2).any(|w| w[0] == "-video_fullscreen" && w[1] == "0"));
    }
}
