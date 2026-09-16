fn main() {
    emit_probe_dependency_versions();
    tauri_build::build();
}

fn emit_probe_dependency_versions() {
    println!("cargo::rerun-if-changed=../Cargo.lock");
    let lock_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", lock_path.display()));

    for (package, variable) in [
        ("tauri", "PROBE_TAURI_VERSION"),
        ("wry", "PROBE_WRY_VERSION"),
        ("webview2-com", "PROBE_WEBVIEW2_COM_VERSION"),
    ] {
        let versions: Vec<_> = lock
            .split("[[package]]")
            .filter(|entry| lock_field(entry, "name") == Some(package))
            .filter_map(|entry| lock_field(entry, "version"))
            .collect();
        let [version] = versions.as_slice() else {
            panic!("{package} must have exactly one version in the resolved dependency graph");
        };
        println!("cargo::rustc-env={variable}={version}");
    }
}

fn lock_field<'a>(entry: &'a str, field: &str) -> Option<&'a str> {
    entry.lines().find_map(|line| {
        line.strip_prefix(field)
            .and_then(|value| value.strip_prefix(" = \""))
            .and_then(|value| value.strip_suffix('"'))
    })
}
