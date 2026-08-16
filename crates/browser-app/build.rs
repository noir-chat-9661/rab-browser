//! Rebuilds `base-ui/dist/index.html` automatically so a stale chrome UI
//! (missing new features, or worse, subtly broken rendering) never ships
//! just because someone forgot to run `pnpm --dir base-ui build` by hand
//! after pulling changes that touch base-ui.

use std::{path::Path, process::Command};

fn main() {
    let base_ui_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../base-ui");

    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("index.html").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("vite.config.ts").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("tsconfig.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        base_ui_dir.join("pnpm-lock.yaml").display()
    );

    if !base_ui_dir.exists() {
        return;
    }

    if which_pnpm().is_err() {
        println!(
            "cargo:warning=pnpm not found on PATH; skipping automatic base-ui build. \
             Run `pnpm --dir base-ui install && pnpm --dir base-ui build` manually, \
             or the chrome UI will be stale/missing."
        );
        return;
    }

    // Always run install (not just when node_modules is missing): pnpm skips
    // reinstalling unchanged deps in well under a second, and this build.rs
    // only runs at all when base-ui's watched files changed (rerun-if-changed
    // above), so the cost is paid rarely and buys correctness when
    // package.json/pnpm-lock.yaml changed but node_modules was left stale.
    run(&["install", "--frozen-lockfile"], &base_ui_dir, "install");

    run(&["run", "build"], &base_ui_dir, "build");
}

fn which_pnpm() -> Result<(), ()> {
    Command::new("pnpm")
        .arg("--version")
        .output()
        .map(|_| ())
        .map_err(|_| ())
}

fn run(args: &[&str], dir: &Path, label: &str) {
    // build.rs always runs non-interactively (no TTY). Without CI=1, pnpm's
    // "confirm modules purge" prompt has nothing to answer and pnpm aborts
    // instead: https://github.com/pnpm/pnpm/issues (ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY).
    match Command::new("pnpm")
        .args(args)
        .current_dir(dir)
        .env("CI", "1")
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => {
            println!("cargo:warning=base-ui {label} exited with {status}; chrome UI may be stale");
        }
        Err(error) => {
            println!("cargo:warning=failed to run base-ui {label}: {error}");
        }
    }
}
