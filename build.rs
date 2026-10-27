#[path = "build_support/version.rs"]
mod version;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support/version.rs");
    println!("cargo:rerun-if-changed=build_support");
    println!("cargo:rerun-if-changed=tests");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    watch_tree(&root.join("src"));

    let output = root.join("output");
    assert_no_symlink(&output);
    fs::create_dir_all(&output).expect("create output directory for version state");
    let state_dir = output.join(".state");
    assert_no_symlink(&state_dir);
    fs::create_dir_all(&state_dir).expect("create output state directory");
    let state = state_dir.join("program-version.txt");
    let previous_state = output.join("program-version.txt");
    assert_no_symlink(&state);
    if !state.exists() && fs::symlink_metadata(&previous_state).is_ok() {
        assert_no_symlink(&previous_state);
        assert!(
            previous_state.is_file(),
            "previous program version state is not a file"
        );
        fs::rename(&previous_state, &state).expect("migrate previous local version state");
    }
    let lock_path = state_dir.join("program-version.lock");
    let _lock = acquire_lock(&lock_path);
    let template = root.join("input/program-version.txt");
    println!("cargo:rerun-if-changed={}", template.display());
    let current_text = fs::read_to_string(&state)
        .or_else(|_| fs::read_to_string(&template))
        .expect("read current or template program version");
    let current = version::Version::parse(&current_text)
        .unwrap_or_else(|error| panic!("invalid program version: {error}"));
    let next = current.increment().to_string();
    let pending = state_dir.join(format!("program-version-{}.tmp", std::process::id()));
    let mut pending_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .expect("create next program version state");
    writeln!(pending_file, "{next}").expect("write next program version state");
    pending_file
        .sync_all()
        .expect("flush next program version state");
    fs::rename(&pending, &state).expect("publish next program version");
    println!("cargo:rustc-env=RUST_FITS_AUTO_CROPPING_VERSION={next}");
    println!("cargo:warning=rust_fits_auto_cropping version {next}");
}

fn assert_no_symlink(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        panic!(
            "managed output directory must not be a symbolic link: {}",
            path.display()
        );
    }
}

fn acquire_lock(path: &Path) -> Lock {
    let started = Instant::now();
    loop {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", std::process::id());
                return Lock(path.to_path_buf());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "timed out waiting for version lock"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("acquire program version lock: {error}"),
        }
    }
}

fn watch_tree(directory: &Path) {
    println!("cargo:rerun-if-changed={}", directory.display());
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                watch_tree(&path);
            } else {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
