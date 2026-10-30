use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let schema_source = manifest.join("schemas");
    let schema_target = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("schemas");

    for path in xml_files(&schema_source) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-changed={}", schema_source.display());

    fs::create_dir_all(&schema_target).expect("create schema target directory");
    let status = Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg("--targetdir")
        .arg(&schema_target)
        .arg(&schema_source)
        .status()
        .expect("glib-compile-schemas is required to build WayToClip");
    assert!(status.success(), "glib-compile-schemas failed");
}

fn xml_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "xml"))
        .collect()
}
