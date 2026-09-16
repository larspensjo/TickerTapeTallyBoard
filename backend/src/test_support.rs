use std::path::{Path, PathBuf};

pub(crate) fn workspace_target_path(path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend crate should have a workspace parent")
        .join("target")
        .join(path)
}
