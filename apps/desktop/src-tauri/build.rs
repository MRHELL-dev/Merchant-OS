use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_64bit_windres() -> Option<PathBuf> {
    // 1. Honor explicit WINDRES environment variable if valid
    if let Ok(env_windres) = std::env::var("WINDRES") {
        let p = PathBuf::from(env_windres);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Generic Windows WinLibs discovery via %LOCALAPPDATA% (portable across any user profile)
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_packages = Path::new(&local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if winget_packages.exists() {
            if let Ok(entries) = fs::read_dir(&winget_packages) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("BrechtSanders.WinLibs.") {
                        let candidate = entry.path().join("mingw64").join("bin").join("windres.exe");
                        if candidate.exists() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }

    // 3. Generic MSYS2 / MinGW environment prefixes
    for prefix_var in &["MINGW_PREFIX", "MSYSTEM_PREFIX"] {
        if let Ok(prefix) = std::env::var(prefix_var) {
            let candidate = Path::new(&prefix).join("bin").join("windres.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let manifest_rc = manifest_dir.join("manifest.rc");
    let manifest_xml = manifest_dir.join("manifest.xml");
    let manifest_o = manifest_dir.join("manifest.o");

    println!("cargo:rerun-if-changed={}", manifest_rc.display());
    println!("cargo:rerun-if-changed={}", manifest_xml.display());

    if let Some(windres) = find_64bit_windres() {
        std::env::set_var("WINDRES", &windres);
        if let Some(parent) = windres.parent() {
            if let Ok(current_path) = std::env::var("PATH") {
                std::env::set_var("PATH", format!("{};{}", parent.display(), current_path));
            }
        }

        // Compile manifest.rc to manifest.o if needed
        if manifest_rc.exists() {
            let should_compile = !manifest_o.exists() || {
                let rc_time = fs::metadata(&manifest_rc).and_then(|m| m.modified()).ok();
                let o_time = fs::metadata(&manifest_o).and_then(|m| m.modified()).ok();
                match (rc_time, o_time) {
                    (Some(rc), Some(o)) => rc > o,
                    _ => true,
                }
            };
            if should_compile {
                let status = Command::new(&windres)
                    .args(["-i", manifest_rc.to_str().unwrap(), "-o", manifest_o.to_str().unwrap(), "-F", "pe-x86-64"])
                    .status();
                if let Err(e) = status {
                    eprintln!("cargo:warning=Failed to compile manifest.rc: {e}");
                }
            }
        }
    }

    // Link manifest.o portably to binaries and integration tests
    if manifest_o.exists() {
        println!("cargo:rustc-link-arg={}", manifest_o.display());
    }

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
