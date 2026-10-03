use crate::AssetOptions;
use const_serialize::{ConstStr, SerializeConst, deserialize_const};
use std::{fmt::Debug, hash::Hash, path::PathBuf};

/// An asset that should be copied by the bundler with some options. This type will be
/// serialized into the binary.
/// CLIs that support manganis, should pull out the assets from the link section, optimize,
/// and write them to the filesystem at [`BundledAsset::bundled_path`] for the application
/// to use.
#[derive(Debug, Eq, Clone, Copy, SerializeConst, serde::Serialize, serde::Deserialize)]
pub struct BundledAsset {
    /// The absolute path of the asset
    absolute_source_path: ConstStr,

    /// The bundled path of the asset
    bundled_path: ConstStr,

    /// The options for the asset
    options: AssetOptions,
}

impl PartialEq for BundledAsset {
    fn eq(&self, other: &Self) -> bool {
        self.absolute_source_path == other.absolute_source_path
            && self.bundled_path == other.bundled_path
            && self.options == other.options
    }
}

impl PartialOrd for BundledAsset {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self
            .absolute_source_path
            .partial_cmp(&other.absolute_source_path)
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.bundled_path.partial_cmp(&other.bundled_path) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.options.partial_cmp(&other.options)
    }
}

impl Hash for BundledAsset {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.absolute_source_path.hash(state);
        self.bundled_path.hash(state);
        self.options.hash(state);
    }
}

impl BundledAsset {
    pub const PLACEHOLDER_HASH: &str = "This should be replaced by dx as part of the build process. If you see this error, make sure you are using a matching version of dx and dioxus and you are not stripping symbols from your binary.";

    #[doc(hidden)]
    /// This should only be called from the macro
    /// Create a new asset
    pub const fn new(
        absolute_source_path: &str,
        bundled_path: &str,
        options: AssetOptions,
    ) -> Self {
        Self {
            absolute_source_path: ConstStr::new(absolute_source_path),
            bundled_path: ConstStr::new(bundled_path),
            options,
        }
    }

    /// Get the bundled name of the asset. This identifier cannot be used to read the asset directly
    pub fn bundled_path(&self) -> &str {
        self.bundled_path.as_str()
    }

    /// Get the absolute path of the asset source. This path will not be available when the asset is bundled
    ///
    /// An asset in its package records a path relative to the package instead, in the form
    /// `cargo-pkg:<name>@<version>/<path>`; see [`BundledAsset::package_relative_source`].
    pub fn absolute_source_path(&self) -> &str {
        self.absolute_source_path.as_str()
    }

    /// Starts a source path recorded relative to its package.
    pub const PACKAGE_PREFIX: &str = "cargo-pkg:";

    /// The package (`<name>@<version>`) and the path in it, when the source path is recorded
    /// relative to its package.
    pub fn package_relative_source(&self) -> Option<(&str, &str)> {
        self.absolute_source_path()
            .strip_prefix(Self::PACKAGE_PREFIX)?
            .split_once('/')
    }

    /// This asset with its source at `absolute_source_path`.
    pub fn with_absolute_source_path(&self, absolute_source_path: &str) -> Self {
        Self::new(absolute_source_path, self.bundled_path(), self.options)
    }

    /// Get the options for the asset
    pub const fn options(&self) -> &AssetOptions {
        &self.options
    }
}

/// A bundled asset with some options. The asset can be used in rsx! to reference the asset.
/// It should not be read directly with [`std::fs::read`] because the path needs to be resolved
/// relative to the bundle
///
/// ```rust, ignore
/// # use manganis::{asset, Asset};
/// # use dioxus::prelude::*;
/// const ASSET: Asset = asset!("/assets/image.png");
/// rsx! {
///     img { src: ASSET }
/// };
/// ```
#[allow(unpredictable_function_pointer_comparisons)]
#[derive(PartialEq, Clone, Copy)]
pub struct Asset {
    /// A function that returns a pointer to the bundled asset. This will be resolved after the linker has run and
    /// put into the lazy asset. We use a function instead of using the pointer directly to force the compiler to
    /// read the static __LINK_SECTION at runtime which will be offset by the hot reloading engine instead
    /// of at compile time which can't be offset
    ///
    /// WARNING: Don't read this directly. Reads can get optimized away at compile time before
    /// the data for this is filled in by the CLI after the binary is built. Instead, use
    /// [`std::ptr::read_volatile`] to read the data.
    bundled: fn() -> &'static [u8],
}

impl Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.resolve().fmt(f)
    }
}

unsafe impl Send for Asset {}
unsafe impl Sync for Asset {}

impl Asset {
    #[doc(hidden)]
    /// This should only be called from the macro
    /// Create a new asset from the bundled form of the asset and the link section
    pub const fn new(bundled: extern "Rust" fn() -> &'static [u8]) -> Self {
        Self { bundled }
    }

    /// Get the bundled asset
    pub fn bundled(&self) -> BundledAsset {
        // Read the slice using volatile reads to prevent the compiler from optimizing
        // away the read at compile time
        let bundled = (self.bundled)();
        let ptr = bundled as *const [u8] as *const u8;
        let len = bundled.len();
        if ptr.is_null() {
            panic!(
                "Tried to use an asset that was not bundled. Make sure you are compiling dx as the linker"
            );
        }
        let mut bytes = Vec::with_capacity(len);
        for byte in 0..len {
            // SAFETY: We checked that the pointer was not null above. The pointer is valid for reads and
            // since we are reading a u8 there are no alignment requirements
            let byte = unsafe { std::ptr::read_volatile(ptr.add(byte)) };
            bytes.push(byte);
        }

        deserialize_const!(BundledAsset, bytes.as_slice()).expect("Failed to deserialize asset. Make sure you built with the matching version of the Dioxus CLI").1
    }

    /// Return a canonicalized path to the asset
    ///
    /// Attempts to resolve it against an `assets` folder in the current directory.
    /// If that doesn't exist, it will resolve against the cargo manifest dir
    pub fn resolve(&self) -> PathBuf {
        #[cfg(feature = "dioxus")]
        // If the asset is relative, we resolve the asset at the current directory
        if !dioxus_core_types::is_bundled_app() {
            let bundled = self.bundled();
            let package_relative = bundled
                .package_relative_source()
                .and_then(|(package, path)| Some(unbundled::package_dir(package)?.join(path)));
            if let Some(path) = package_relative {
                return path;
            }
            return PathBuf::from(bundled.absolute_source_path.as_str());
        }

        #[cfg(feature = "dioxus")]
        let bundle_root = {
            let base_path = dioxus_cli_config::base_path();
            let base_path = base_path
                .as_deref()
                .map(|base_path| {
                    let trimmed = base_path.trim_matches('/');
                    format!("/{trimmed}")
                })
                .unwrap_or_default();
            PathBuf::from(format!("{base_path}/assets/"))
        };
        #[cfg(not(feature = "dioxus"))]
        let bundle_root = PathBuf::from("/assets/");

        // Otherwise presumably we're bundled and we can use the bundled path
        bundle_root.join(PathBuf::from(
            self.bundled().bundled_path.as_str().trim_start_matches('/'),
        ))
    }
}

impl From<Asset> for String {
    fn from(value: Asset) -> Self {
        value.to_string()
    }
}
impl From<Asset> for Option<String> {
    fn from(value: Asset) -> Self {
        Some(value.to_string())
    }
}

impl std::fmt::Display for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.resolve().display())
    }
}

#[cfg(feature = "dioxus")]
impl dioxus_core_types::DioxusFormattable for Asset {
    fn format(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

/// Package directories for an app run by `cargo run` or `cargo test`, which set `CARGO`,
/// `CARGO_MANIFEST_DIR`, `CARGO_PKG_NAME` and `CARGO_PKG_VERSION`.
#[cfg(feature = "dioxus")]
mod unbundled {
    use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

    /// The directory of `package` (`<name>@<version>`): the running package's own, else the one
    /// `cargo metadata` names, asked once.
    pub(super) fn package_dir(package: &str) -> Option<PathBuf> {
        let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
        let running = format!(
            "{}@{}",
            std::env::var("CARGO_PKG_NAME").ok()?,
            std::env::var("CARGO_PKG_VERSION").ok()?
        );
        if package == running {
            return Some(manifest_dir);
        }
        static DIRS: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();
        DIRS.get_or_init(|| metadata_dirs(&manifest_dir).unwrap_or_default())
            .get(package)
            .cloned()
    }

    fn metadata_dirs(manifest_dir: &std::path::Path) -> Option<HashMap<String, PathBuf>> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = std::process::Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(manifest_dir.join("Cargo.toml"))
            .output()
            .ok()?;
        dirs_from_metadata(&output.stdout)
    }

    /// Package directories by `<name>@<version>`, from `cargo metadata --format-version 1`.
    fn dirs_from_metadata(json: &[u8]) -> Option<HashMap<String, PathBuf>> {
        let metadata: serde_json::Value = serde_json::from_slice(json).ok()?;
        let dirs = metadata["packages"]
            .as_array()?
            .iter()
            .filter_map(|package| {
                let manifest = PathBuf::from(package["manifest_path"].as_str()?);
                Some((
                    format!(
                        "{}@{}",
                        package["name"].as_str()?,
                        package["version"].as_str()?
                    ),
                    manifest.parent()?.to_path_buf(),
                ))
            })
            .collect();
        Some(dirs)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn metadata_gives_each_package_its_directory() {
            let json = br#"{"packages": [
                {"name": "app", "version": "0.1.0", "manifest_path": "/w/app/Cargo.toml"},
                {"name": "dep", "version": "2.0.0", "manifest_path": "/r/dep-2.0.0/Cargo.toml"}
            ]}"#;
            let dirs = dirs_from_metadata(json).unwrap();
            assert_eq!(dirs["app@0.1.0"], PathBuf::from("/w/app"));
            assert_eq!(dirs["dep@2.0.0"], PathBuf::from("/r/dep-2.0.0"));
        }

        #[test]
        fn cargo_metadata_finds_a_package() {
            let dir = tempfile::tempdir().unwrap();
            // `cargo metadata` reports the canonical path (macOS links /var to /private/var).
            let manifest_dir = std::fs::canonicalize(dir.path()).unwrap();
            std::fs::write(
                manifest_dir.join("Cargo.toml"),
                "[package]\nname = \"probe\"\nversion = \"1.2.3\"\nedition = \"2021\"\n\n[workspace]\n",
            )
            .unwrap();
            std::fs::create_dir(manifest_dir.join("src")).unwrap();
            std::fs::write(manifest_dir.join("src/lib.rs"), "").unwrap();
            let dirs = metadata_dirs(&manifest_dir).unwrap();
            assert_eq!(dirs["probe@1.2.3"], manifest_dir);
        }
    }
}
