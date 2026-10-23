//! Bakes the commit the binary was built from into `LANDAV_COMMIT`.
//!
//! `landav bench run` names a baseline after the code that produced it
//! (`LAN-123`), and the only honest source of that name is the build. A tree
//! with uncommitted changes gets `-dirty` appended, so a baseline from code
//! nobody can check out is never mistaken for the commit's own. Outside a git
//! checkout - a source tarball, a vendored build - the value is `unknown`,
//! and a caller that wants a better name passes `--label`.
//!
//! `LANDAV_COMMIT` set in the build environment wins over `git`, for a CI
//! that knows its own revision better than a shallow clone does.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-env-changed=LANDAV_COMMIT");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    let commit = std::env::var("LANDAV_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let short = git(&["rev-parse", "--short", "HEAD"])?;
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|status| !status.is_empty());
            Some(if dirty {
                format!("{short}-dirty")
            } else {
                short
            })
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=LANDAV_COMMIT={commit}");
}
