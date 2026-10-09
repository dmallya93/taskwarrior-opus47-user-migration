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

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TASK_COMMIT");

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
