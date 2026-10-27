//! Safe creation and path derivation for one isolated processing run.
use crate::durability::{sync_directory, sync_parent};
use anyhow::{Context, bail};
use std::{
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct OutputLayout {
    root: PathBuf,
    run: PathBuf,
}

impl OutputLayout {
    pub fn prepare(requested: &Path, input_root: &Path, run_id: &str) -> anyhow::Result<Self> {
        if run_id.is_empty()
            || run_id == "."
            || run_id == ".."
            || !run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            bail!("run identifier contains unsafe path characters");
        }
        if fs::symlink_metadata(requested).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            bail!("output directory must not be a symbolic link");
        }
        let resolved = resolve_destination(requested)?;
        if input_root.starts_with(&resolved) || resolved.starts_with(input_root) {
            bail!("input and output directories must not overlap");
        }
        fs::create_dir_all(requested)
            .with_context(|| format!("creating output directory {}", requested.display()))?;
        let root = fs::canonicalize(requested)?;
        if !root.is_dir() {
            bail!("output path is not a directory");
        }
        if input_root.starts_with(&root) || root.starts_with(input_root) {
            bail!("input and output directories must not overlap");
        }
        sync_parent(&root)?;

        let runs = root.join("runs");
        ensure_plain_directory(&runs)?;
        let run = runs.join(run_id);
        fs::create_dir(&run)
            .with_context(|| format!("creating unique run output directory {}", run.display()))?;
        sync_parent(&run)?;
        for name in ["images", "masks", "previews", "manifests"] {
            fs::create_dir(run.join(name))?;
        }
        sync_directory(&run)?;
        Ok(Self { root, run })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn run(&self) -> &Path {
        &self.run
    }

    /// Resolve an input-relative destination after validating every component.
    pub fn product_path(
        &self,
        category: ProductCategory,
        relative: &Path,
    ) -> anyhow::Result<PathBuf> {
        validate_relative(relative)?;
        let base = self.run.join(category.directory());
        ensure_plain_directory(&base)?;
        let mut parent = base.clone();
        let components: Vec<_> = relative.components().collect();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            let Component::Normal(name) = component else {
                unreachable!()
            };
            parent.push(name);
            ensure_plain_directory(&parent)?;
        }
        Ok(base.join(relative))
    }

    pub fn preview_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        let path = self.product_path(ProductCategory::Preview, relative)?;
        let mut filename = OsString::from(path.as_os_str());
        filename.push(".png");
        Ok(PathBuf::from(filename))
    }

    pub fn reason_mask_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        self.product_path(ProductCategory::ReasonMask, relative)
    }

    pub fn manifest_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        let path = self.product_path(ProductCategory::Manifest, relative)?;
        let mut filename = OsString::from(path.as_os_str());
        filename.push(".complete.json");
        Ok(PathBuf::from(filename))
    }
}

#[derive(Clone, Copy)]
pub enum ProductCategory {
    Image,
    Mask,
    Preview,
    Manifest,
    ReasonMask,
}

impl ProductCategory {
    fn directory(self) -> &'static str {
        match self {
            Self::Image => "images",
            Self::Mask => "masks",
            Self::Preview => "previews",
            Self::Manifest => "manifests",
            Self::ReasonMask => "reason_masks",
        }
    }
}

fn validate_relative(path: &Path) -> anyhow::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("input-relative output path contains an unsafe component");
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            value => normalized.push(value.as_os_str()),
        }
    }

    let mut existing = normalized.clone();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .context("output path has no existing parent directory")?;
        suffix.push(name.to_os_string());
        existing.pop();
    }
    let mut resolved = fs::canonicalize(existing)?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn ensure_plain_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "output path component must not be a symbolic link: {}",
                path.display()
            )
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!(
                "output path component is not a directory: {}",
                path.display()
            )
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {
                sync_parent(path)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure_plain_directory(path)
            }
            Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
        },
        Err(error) => Err(error).with_context(|| format!("checking {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prepares_one_consolidated_run_tree_and_nested_destinations() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir(&input).unwrap();
        fs::create_dir(&output).unwrap();
        fs::write(output.join("user-note.txt"), "preserved").unwrap();
        let layout = OutputLayout::prepare(&output, &input, "20261027T120000000Z").unwrap();
        assert_eq!(
            layout
                .product_path(ProductCategory::Image, Path::new("night/frame.fits"))
                .unwrap(),
            output.join("runs/20261027T120000000Z/images/night/frame.fits")
        );
        assert!(layout.run().join("masks").is_dir());
        assert!(layout.run().join("previews").is_dir());
        assert!(layout.run().join("manifests").is_dir());
        assert_eq!(
            layout.manifest_path(Path::new("night/frame.fits")).unwrap(),
            output.join("runs/20261027T120000000Z/manifests/night/frame.fits.complete.json")
        );
        assert_eq!(
            fs::read_to_string(output.join("user-note.txt")).unwrap(),
            "preserved"
        );
        assert!(OutputLayout::prepare(&output, &input, "20261027T120000000Z").is_err());
    }

    #[test]
    fn rejects_output_symlink_overlap_unsafe_relative_paths_and_symlink_components() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir(&input).unwrap();
        fs::create_dir(&output).unwrap();
        let linked = temp.path().join("linked-output");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&output, &linked).unwrap();
        #[cfg(unix)]
        assert!(OutputLayout::prepare(&linked, &input, "r1").is_err());
        assert!(OutputLayout::prepare(&input, &input, "r2").is_err());
        let nested = input.join("new-output");
        assert!(OutputLayout::prepare(&nested, &input, "r2b").is_err());
        assert!(!nested.exists());
        let layout = OutputLayout::prepare(&output, &input, "r3").unwrap();
        assert!(
            layout
                .product_path(ProductCategory::Image, Path::new("../escape.fits"))
                .is_err()
        );
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside");
            fs::create_dir(&outside).unwrap();
            std::os::unix::fs::symlink(outside, layout.run().join("images/linked")).unwrap();
            assert!(
                layout
                    .product_path(ProductCategory::Image, Path::new("linked/file.fits"))
                    .is_err()
            );
        }
    }
}
