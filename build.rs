use std::{env, fs};

const PRESERVED_SCHEMA_FILES: &[&str] = &[
    "gen/schemas/acl-manifests.json",
    "gen/schemas/desktop-schema.json",
];

fn main() {
    let preserve_schema_files = env::var("CARGO_CFG_TARGET_OS")
        .map(|target_os| target_os != "macos")
        .unwrap_or(false);
    let schema_snapshots = if preserve_schema_files {
        // tauri-build rewrites desktop ACL schemas for the active platform.
        // Keep the committed macOS-flavored schemas stable during Linux checks.
        capture_schema_files()
    } else {
        Vec::new()
    };
    let linux_schema_existed = fs::exists("gen/schemas/linux-schema.json").unwrap_or(false);

    tauri_build::build();

    if preserve_schema_files {
        restore_schema_files(schema_snapshots);
        if !linux_schema_existed {
            remove_generated_file("gen/schemas/linux-schema.json");
        }
    }
}

fn capture_schema_files() -> Vec<(&'static str, Option<Vec<u8>>)> {
    PRESERVED_SCHEMA_FILES
        .iter()
        .copied()
        .map(|path| (path, fs::read(path).ok()))
        .collect()
}

fn restore_schema_files(snapshots: Vec<(&'static str, Option<Vec<u8>>)>) {
    for (path, contents) in snapshots {
        let result = match contents {
            Some(contents) => fs::write(path, contents),
            None => fs::remove_file(path).or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            }),
        };

        if let Err(error) = result {
            println!("cargo:warning=failed to restore generated schema file {path}: {error}");
        }
    }
}

fn remove_generated_file(path: &str) {
    if fs::exists(path).unwrap_or(false) {
        let _ = fs::remove_file(path);
    }
}
