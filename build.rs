use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

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
        // Keep the committed macOS-flavored schemas stable during non-macOS
        // checks.
        capture_schema_files()
    } else {
        Vec::new()
    };
    let preexisting_platform_schema_files = if preserve_schema_files {
        platform_schema_files()
    } else {
        BTreeSet::new()
    };

    tauri_build::build();

    if preserve_schema_files {
        restore_schema_files(schema_snapshots);
        remove_new_platform_schema_files(preexisting_platform_schema_files);
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

fn platform_schema_files() -> BTreeSet<PathBuf> {
    let Ok(entries) = fs::read_dir("gen/schemas") else {
        return BTreeSet::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with("-schema.json"))
                .unwrap_or(false)
        })
        .collect()
}

fn remove_new_platform_schema_files(preexisting_files: BTreeSet<PathBuf>) {
    for path in platform_schema_files() {
        if !preexisting_files.contains(&path) {
            remove_generated_file(&path);
        }
    }
}

fn remove_generated_file(path: &Path) {
    if fs::exists(path).unwrap_or(false) {
        let _ = fs::remove_file(path);
    }
}
