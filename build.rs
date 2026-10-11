//! Build script: mirrors the `HAVE_COMMIT` / `COMMIT` detection of the C++
//! `CMakeLists.txt`:
//!
//! ```cmake
//! if (EXISTS ${CMAKE_SOURCE_DIR}/.git/index)
//!   set (HAVE_COMMIT true)
//!   execute_process (COMMAND git log -1 --pretty=format:%h --no-show-signature
//!                    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
//!                    OUTPUT_VARIABLE COMMIT)
//! ```
//!
//! When `.git/index` exists in the package directory, the (possibly empty)
//! output of that git command is exported as the compile-time environment
//! variable `TASK_COMMIT`, read by `src/version.rs`. Otherwise nothing is
//! exported (no commit; a `TASK_COMMIT` from the build environment is still
//! honoured).
//!
//! It also exports the Rust equivalents of the compiler and build facts the
//! C++ `task diagnostics` takes from preprocessor macros (`__VERSION__`,
//! `CMAKE_VERSION`, `CMAKE_BUILD_TYPE`): `TASK_RUSTC_VERSION` (`$RUSTC
//! --version`), `TASK_CARGO_VERSION` (`$CARGO --version`), `TASK_TARGET`
//! (target triple), `TASK_PROFILE` (cargo profile) and `TASK_FEATURES` (the
//! enabled cargo features, comma separated, from `CARGO_FEATURE_*`).

use std::path::Path;
use std::process::Command;

/// The first line of `<program> --version`, or `""` if it cannot run.
fn tool_version(program: &str) -> String {
    Command::new(program)
        .arg("--version")
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

/// Exports the compiler / build facts reported by `task diagnostics`.
fn export_build_facts() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    println!("cargo:rustc-env=TASK_RUSTC_VERSION={}", tool_version(&rustc));
    println!("cargo:rustc-env=TASK_CARGO_VERSION={}", tool_version(&cargo));
    println!(
        "cargo:rustc-env=TASK_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=TASK_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );

    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| {
            key.strip_prefix("CARGO_FEATURE_")
                .map(|name| name.to_ascii_lowercase().replace('_', "-"))
        })
        .collect();
    features.sort();
    println!("cargo:rustc-env=TASK_FEATURES={}", features.join(","));
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TASK_COMMIT");

    export_build_facts();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let root = Path::new(&manifest_dir);
    let git_dir = root.join(".git");

    if !git_dir.join("index").exists() {
        return;
    }

    // Stay fresh: HEAD moves on checkout, the branch ref and the index on commit.
    for file in ["HEAD", "index", "packed-refs"] {
        let path = git_dir.join(file);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            let path = git_dir.join(reference);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    // CMake's OUTPUT_VARIABLE is empty when git cannot run.
    let commit = Command::new("git")
        .args(["log", "-1", "--pretty=format:%h", "--no-show-signature"])
        .current_dir(root)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();

    println!("cargo:rustc-env=TASK_COMMIT={commit}");
}
