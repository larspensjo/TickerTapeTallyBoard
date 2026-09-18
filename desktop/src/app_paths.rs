use std::path::{Path, PathBuf};

/// The undistributed desktop shell serves assets from its checked-out build tree.
const DESKTOP_CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub static_assets_dir: PathBuf,
}

impl AppPaths {
    pub fn from_environment() -> Self {
        Self::from_anchor_and_override(
            Path::new(DESKTOP_CRATE_DIR),
            std::env::var_os("TTTB_STATIC_DIR").map(PathBuf::from),
        )
    }

    pub fn from_anchor_and_override(anchor: &Path, override_dir: Option<PathBuf>) -> Self {
        let default = anchor.join("..").join("frontend").join("dist");
        let static_assets_dir = match override_dir {
            Some(path) if path.is_absolute() => path,
            Some(path) => anchor.join(path),
            None => default,
        };
        Self { static_assets_dir }
    }
}

#[cfg(test)]
mod tests {
    use super::AppPaths;
    use std::path::{Path, PathBuf};

    #[test]
    fn default_assets_derive_from_the_supplied_build_tree_anchor() {
        let paths = AppPaths::from_anchor_and_override(Path::new("C:/source/desktop"), None);

        assert_eq!(
            paths.static_assets_dir,
            PathBuf::from("C:/source/desktop/../frontend/dist")
        );
    }

    #[test]
    fn relative_override_is_anchored_to_the_build_tree_not_the_current_directory() {
        let paths = AppPaths::from_anchor_and_override(
            Path::new("C:/source/desktop"),
            Some(PathBuf::from("fixtures/dist")),
        );

        assert_eq!(
            paths.static_assets_dir,
            PathBuf::from("C:/source/desktop/fixtures/dist")
        );
    }

    #[test]
    fn absolute_override_wins() {
        let paths = AppPaths::from_anchor_and_override(
            Path::new("C:/source/desktop"),
            Some(PathBuf::from("D:/assets/dist")),
        );

        assert_eq!(paths.static_assets_dir, PathBuf::from("D:/assets/dist"));
    }
}
