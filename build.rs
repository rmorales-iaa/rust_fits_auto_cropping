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
    fs::create_dir_all(&output).expect("create output directory for version state");
    let state = output.join("program-version.txt");
    let lock_path = output.join("program-version.lock");
    let _lock = acquire_lock(&lock_path);
    let template = root.join("input/program-version.txt");
    println!("cargo:rerun-if-changed={}", template.display());
    let current_text = fs::read_to_string(&state)
        .or_else(|_| fs::read_to_string(&template))
        .expect("read current or template program version");
    let current = version::Version::parse(&current_text)
        .unwrap_or_else(|error| panic!("invalid program version: {error}"));
    let next = current.increment().to_string();
    let pending = output.join(format!("program-version-{}.tmp", std::process::id()));
    fs::write(&pending, format!("{next}\n")).expect("write next program version");
    fs::rename(&pending, &state).expect("publish next program version");
    println!("cargo:rustc-env=FITS_AUTO_CLIPPING_VERSION={next}");
    println!("cargo:warning=fits-auto-clipping version {next}");
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
