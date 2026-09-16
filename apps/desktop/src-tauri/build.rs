use std::fs;
use std::path::PathBuf;

fn main() {
    let windows_attrs = tauri_build::WindowsAttributes::new_without_app_manifest();
    let attrs = tauri_build::Attributes::new().windows_attributes(windows_attrs);
    tauri_build::try_build(attrs).expect("failed to build tauri attributes");

    // Ensure WebView2Loader.dll is copied to the deps directory for test runners if available
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out_path = PathBuf::from(out_dir);
        // OUT_DIR is typically target/debug/build/<pkg>/out
        // Find target/debug/deps
        if let Some(target_debug) = out_path.ancestors().nth(3) {
            let src_dll = target_debug.join("WebView2Loader.dll");
            let deps_dir = target_debug.join("deps");
            let dst_dll = deps_dir.join("WebView2Loader.dll");

            if src_dll.exists() && deps_dir.exists() && !dst_dll.exists() {
                let _ = fs::copy(&src_dll, &dst_dll);
            }
        }
    }
}
