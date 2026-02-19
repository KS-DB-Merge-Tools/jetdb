use std::path::PathBuf;
use std::process::Command;

/// Resolve the path to a test data file, returning `None` if missing.
pub fn test_data_path(relative: &str) -> Option<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir)
        .join("../../testdata")
        .join(relative);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

macro_rules! skip_if_missing {
    ($path:expr) => {
        match common::test_data_path($path) {
            Some(p) => p,
            None => {
                eprintln!("SKIP: test data not found: {}", $path);
                return;
            }
        }
    };
}

pub fn jetdb_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetdb"))
}
