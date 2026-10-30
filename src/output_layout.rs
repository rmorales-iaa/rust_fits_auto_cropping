//! Safe creation and path derivation for one isolated processing run.
use crate::durability::{sync_directory, sync_parent};
use anyhow::{Context, bail};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct OutputLayout {
    root: PathBuf,
    run: PathBuf,
    flat_output: bool,
    /// Inputs whose extension-free preview name is shared with another input
    /// in the same directory; their previews keep the full input name.
    preview_name_collisions: HashSet<PathBuf>,
}

/// FITS extensions removed from preview file names (case-insensitive).
const PREVIEW_STRIPPED_EXTENSIONS: [&str; 3] = ["fits", "fit", "fts"];

/// `night/frame.fits` -> `night/frame`; other names are returned unchanged.
fn preview_stem(relative: &Path) -> PathBuf {
    let strip = relative.extension().is_some_and(|extension| {
        PREVIEW_STRIPPED_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    });
    match (strip, relative.file_stem()) {
        (true, Some(stem)) if !stem.is_empty() => relative.with_file_name(stem),
        _ => relative.to_path_buf(),
    }
}

impl OutputLayout {
    pub fn prepare(
        requested: &Path,
        input_root: &Path,
        run_id: &str,
        flat_output: bool,
        sanitize_output: bool,
    ) -> anyhow::Result<Self> {
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
        if sanitize_output {
            clear_directory_contents(&root)?;
        }
        sync_parent(&root)?;

        let run = if flat_output {
            root.clone()
        } else {
            let runs = root.join("runs");
            ensure_plain_directory(&runs)?;
            let run = runs.join(run_id);
            fs::create_dir(&run).with_context(|| {
                format!("creating unique run output directory {}", run.display())
            })?;
            sync_parent(&run)?;
            run
        };
        // Flat mode stores image products at the root; product-level
        // overwrite policy remains controlled by the caller. No mask sidecar
        // directory is created: action masks are verified in memory.
        if !flat_output {
            fs::create_dir(run.join("images"))?;
        }
        sync_directory(&run)?;
        Ok(Self {
            root,
            run,
            flat_output,
            preview_name_collisions: HashSet::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn run(&self) -> &Path {
        &self.run
    }

    /// Remove transient binary action-mask trees before reports are finalized.
    /// These sidecars are used only while writing and checking science images;
    /// they are intentionally not retained as run outputs.
    pub fn remove_transient_mask_directories(&self) -> anyhow::Result<()> {
        for name in ["masks", "reason_masks"] {
            let path = self.run.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    remove_entry_without_following_links(&path)?;
                    sync_parent(&path)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("checking {}", path.display()));
                }
            }
        }
        Ok(())
    }

    /// Resolve an input-relative destination after validating every component.
    pub fn product_path(
        &self,
        category: ProductCategory,
        relative: &Path,
    ) -> anyhow::Result<PathBuf> {
        validate_relative(relative)?;
        let base = match (self.flat_output, category) {
            (true, ProductCategory::Image) => self.run.clone(),
            _ => self.run.join(category.directory()),
        };
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

    /// Register every input-relative path of the run so that inputs sharing an
    /// extension-free preview name (`frame.fits`, `frame.fit`) keep their full
    /// names instead of overwriting each other's preview.
    pub fn register_preview_inputs<'a>(&mut self, relatives: impl IntoIterator<Item = &'a Path>) {
        let mut by_stem: HashMap<PathBuf, Vec<&Path>> = HashMap::new();
        for relative in relatives {
            by_stem
                .entry(preview_stem(relative))
                .or_default()
                .push(relative);
        }
        self.preview_name_collisions = by_stem
            .into_values()
            .filter(|group| group.len() > 1)
            .flatten()
            .map(Path::to_path_buf)
            .collect();
    }

    /// Preview destination: the input-relative path without its FITS
    /// extension plus `.png` (`night/frame.fits` -> `previews/night/frame.png`).
    pub fn preview_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        let name = if self.preview_name_collisions.contains(relative) {
            relative.to_path_buf()
        } else {
            preview_stem(relative)
        };
        let path = self.product_path(ProductCategory::Preview, &name)?;
        let mut filename = OsString::from(path.as_os_str());
        filename.push(".png");
        Ok(PathBuf::from(filename))
    }

    /// Return the science-image destination for a newly generated crop.
    /// Kept and pass-through images retain the original relative name.
    pub fn cropped_image_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        validate_relative(relative)?;
        let file_name = relative
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("crop output path has no file name"))?;
        let mut cropped_name = relative
            .file_stem()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from(file_name));
        cropped_name.push("_cropped");
        if let Some(extension) = relative.extension() {
            cropped_name.push(".");
            cropped_name.push(extension);
        }
        let cropped_relative = relative.with_file_name(cropped_name);
        self.product_path(ProductCategory::Image, &cropped_relative)
    }

    pub fn reason_mask_path(&self, relative: &Path) -> anyhow::Result<PathBuf> {
        self.product_path(ProductCategory::ReasonMask, relative)
    }
}

impl Drop for OutputLayout {
    fn drop(&mut self) {
        // Best-effort fallback for early returns and unwinding. The normal
        // pipeline calls the checked cleanup method and reports any failures.
        for name in ["masks", "reason_masks"] {
            let path = self.run.join(name);
            if fs::symlink_metadata(&path).is_ok() {
                let _ = remove_entry_without_following_links(&path);
            }
        }
    }
}

fn remove_entry_without_following_links(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("removing transient output entry {}", path.display()))
}

fn clear_directory_contents(directory: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory).with_context(|| {
        format!(
            "listing output directory {} for sanitation",
            directory.display()
        )
    })? {
        let path = entry?.path();
        remove_entry_without_following_links(&path)?;
    }
    sync_directory(directory)?;
    Ok(())
}

#[derive(Clone, Copy)]
pub enum ProductCategory {
    Image,
    Preview,
    ReasonMask,
}

impl ProductCategory {
    fn directory(self) -> &'static str {
        match self {
            Self::Image => "images",
            Self::Preview => "previews",
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
        let layout =
            OutputLayout::prepare(&output, &input, "20261027T120000000Z", false, false).unwrap();
        assert_eq!(
            layout
                .product_path(ProductCategory::Image, Path::new("night/frame.fits"))
                .unwrap(),
            output.join("runs/20261027T120000000Z/images/night/frame.fits")
        );
        assert_eq!(
            layout
                .cropped_image_path(Path::new("night/frame.fits"))
                .unwrap(),
            output.join("runs/20261027T120000000Z/images/night/frame_cropped.fits")
        );
        assert!(!layout.run().join("masks").exists());
        assert!(!layout.run().join("previews").exists());
        assert!(!layout.run().join("manifests").exists());
        assert_eq!(
            layout.preview_path(Path::new("night/frame.fits")).unwrap(),
            layout.run().join("previews/night/frame.png")
        );
        assert!(layout.run().join("previews").is_dir());
        assert_eq!(
            fs::read_to_string(output.join("user-note.txt")).unwrap(),
            "preserved"
        );
        assert!(
            OutputLayout::prepare(&output, &input, "20261027T120000000Z", false, false).is_err()
        );
        let direct_output = temp.path().join("direct-output");
        fs::create_dir(&direct_output).unwrap();
        let direct =
            OutputLayout::prepare(&direct_output, &input, "direct-run", true, false).unwrap();
        assert_eq!(direct.run(), fs::canonicalize(&direct_output).unwrap());
        assert!(!direct_output.join("runs").exists());
        assert_eq!(
            direct
                .product_path(ProductCategory::Image, Path::new("night/frame.fits"))
                .unwrap(),
            direct_output.join("night/frame.fits")
        );
        assert_eq!(
            direct
                .cropped_image_path(Path::new("night/frame.fits"))
                .unwrap(),
            direct_output.join("night/frame_cropped.fits")
        );
        assert_eq!(
            direct
                .product_path(ProductCategory::Preview, Path::new("night/frame.fits"))
                .unwrap(),
            direct_output.join("previews/night/frame.fits")
        );
        assert!(!direct_output.join("images").exists());
        let repeated =
            OutputLayout::prepare(&direct_output, &input, "direct-run-2", true, false).unwrap();
        assert_eq!(repeated.run(), direct.run());
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
        assert!(OutputLayout::prepare(&linked, &input, "r1", false, false).is_err());
        assert!(OutputLayout::prepare(&input, &input, "r2", false, false).is_err());
        let nested = input.join("new-output");
        assert!(OutputLayout::prepare(&nested, &input, "r2b", false, false).is_err());
        assert!(!nested.exists());
        let layout = OutputLayout::prepare(&output, &input, "r3", false, false).unwrap();
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

    #[test]
    fn drop_removes_transient_mask_trees_on_early_exit() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir(&input).unwrap();
        let layout = OutputLayout::prepare(&output, &input, "cleanup-test", false, false).unwrap();
        let masks = layout.run().join("masks/nested");
        let reasons = layout.run().join("reason_masks/nested");
        fs::create_dir_all(&masks).unwrap();
        fs::create_dir_all(&reasons).unwrap();
        fs::write(masks.join("mask.fits"), b"temporary").unwrap();
        fs::write(reasons.join("reason.fits"), b"temporary").unwrap();
        let run = layout.run().to_path_buf();

        drop(layout);

        assert!(!run.join("masks").exists());
        assert!(!run.join("reason_masks").exists());
    }

    #[test]
    fn preview_names_drop_the_fits_extension_unless_two_inputs_collide() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir(&input).unwrap();
        let mut layout =
            OutputLayout::prepare(&output, &input, "20261030T120000000Z", false, false).unwrap();
        let inputs = ["a.fits", "a.FIT", "b.fts", "c.fits.gz", "n/a.fits"].map(Path::new);
        layout.register_preview_inputs(inputs);
        let previews = layout.run().join("previews");
        let name = |rel: &str| layout.preview_path(Path::new(rel)).unwrap();
        assert_eq!(name("a.fits"), previews.join("a.fits.png"));
        assert_eq!(name("a.FIT"), previews.join("a.FIT.png"));
        assert_eq!(name("b.fts"), previews.join("b.png"));
        assert_eq!(name("c.fits.gz"), previews.join("c.fits.gz.png"));
        assert_eq!(name("n/a.fits"), previews.join("n/a.png"));
    }
}
