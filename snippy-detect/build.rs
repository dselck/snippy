fn main() {
    #[cfg(target_os = "linux")]
    {
        // When linking ort with -lstdc++, if libstdc++-devel is not installed,
        // libstdc++.so.6 is present but libstdc++.so is missing.
        let candidate_dirs = [
            "/usr/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib",
        ];

        let mut has_dev_lib = false;
        let mut found_so6: Option<&str> = None;

        for dir in candidate_dirs {
            let path_dev = format!("{}/libstdc++.so", dir);
            let path_so6 = format!("{}/libstdc++.so.6", dir);
            if std::path::Path::new(&path_dev).exists() {
                has_dev_lib = true;
                break;
            }
            if found_so6.is_none() && std::path::Path::new(&path_so6).exists() {
                found_so6 = Some(dir);
            }
        }

        if !has_dev_lib {
            if let Some(dir) = found_so6 {
                let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
                let link_target = out_dir.join("libstdc++.so");
                if !link_target.exists() {
                    let source = format!("{}/libstdc++.so.6", dir);
                    let _ = std::os::unix::fs::symlink(source, &link_target);
                }
                println!("cargo:rustc-link-search=native={}", out_dir.display());
            }
        }
    }
}
