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

fn find_webview2_loader(manifest_dir: &Path) -> Option<PathBuf> {
    let target_arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };

    // 1. Check if already staged in manifest_dir
    let manifest_loader = manifest_dir.join("WebView2Loader.dll");
    if manifest_loader.exists() {
        return Some(manifest_loader);
    }

    // 2. Check target_dir (OUT_DIR ancestor)
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out_path = PathBuf::from(out_dir);
        if let Some(target_dir) = out_path.ancestors().nth(3) {
            let candidate = target_dir.join("WebView2Loader.dll");
            if candidate.exists() {
                return Some(candidate);
            }
            if let Ok(entries) = fs::read_dir(target_dir.join("build")) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.to_string_lossy().contains("webview2-com-sys") {
                        let candidate = p.join("out").join(target_arch).join("WebView2Loader.dll");
                        if candidate.exists() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }

    // 3. Search in CARGO_HOME / registry for webview2-com-sys crate
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map(|h| PathBuf::from(h).join(".cargo"))
        });

    if let Ok(cargo_home) = cargo_home {
        let registry_src = cargo_home.join("registry").join("src");
        if let Ok(indices) = fs::read_dir(&registry_src) {
            for index_entry in indices.flatten() {
                if let Ok(crates) = fs::read_dir(index_entry.path()) {
                    for crate_entry in crates.flatten() {
                        let name = crate_entry.file_name();
                        if name.to_string_lossy().starts_with("webview2-com-sys") {
                            let candidate = crate_entry.path().join(target_arch).join("WebView2Loader.dll");
                            if candidate.exists() {
                                return Some(candidate);
                            }
                        }
                    }
                }
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

    // Stage WebView2Loader.dll BEFORE tauri_build validates bundle.resources
    if let Some(loader_source) = find_webview2_loader(&manifest_dir) {
        let manifest_loader = manifest_dir.join("WebView2Loader.dll");
        if loader_source != manifest_loader {
            let _ = fs::copy(&loader_source, &manifest_loader);
        }

        if let Ok(out_dir) = std::env::var("OUT_DIR") {
            let out_path = PathBuf::from(out_dir);
            if let Some(target_dir) = out_path.ancestors().nth(3) {
                let target_loader = target_dir.join("WebView2Loader.dll");
                if loader_source != target_loader && !target_loader.exists() {
                    let _ = fs::copy(&loader_source, &target_loader);
                }
                let deps_dir = target_dir.join("deps");
                let dst_deps = deps_dir.join("WebView2Loader.dll");
                if deps_dir.exists() && !dst_deps.exists() {
                    let _ = fs::copy(&loader_source, &dst_deps);
                }
            }
        }
    }

    let windows_attrs = tauri_build::WindowsAttributes::new_without_app_manifest();
    let attrs = tauri_build::Attributes::new().windows_attributes(windows_attrs);
    tauri_build::try_build(attrs).expect("failed to build tauri attributes");
}
