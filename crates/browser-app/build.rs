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
        ensure_placeholder(&base_ui_dir);
        return;
    }

    if which_pnpm().is_err() {
        println!(
            "cargo:warning=pnpm not found on PATH; skipping automatic base-ui build. \
             Run `pnpm --dir base-ui install && pnpm --dir base-ui build` manually, \
             or the chrome UI will be stale/missing."
        );
        ensure_placeholder(&base_ui_dir);
        return;
    }

    // Always run install (not just when node_modules is missing): pnpm skips
    // reinstalling unchanged deps in well under a second, and this build.rs
    // only runs at all when base-ui's watched files changed (rerun-if-changed
    // above), so the cost is paid rarely and buys correctness when
    // package.json/pnpm-lock.yaml changed but node_modules was left stale.
    run(&["install", "--frozen-lockfile"], &base_ui_dir, "install");

    run(&["run", "build"], &base_ui_dir, "build");

    // include_str!(base-ui/dist/index.html) needs a file to exist at compile
    // time regardless of whether the pnpm build above actually succeeded.
    ensure_placeholder(&base_ui_dir);
}

/// Writes a minimal fallback `dist/index.html` if none exists yet, so
/// `include_str!` in main.rs always has something to embed. Never overwrites
/// a real (or even stale) build output.
fn ensure_placeholder(base_ui_dir: &Path) {
    let dist_dir = base_ui_dir.join("dist");
    let index_path = dist_dir.join("index.html");
    if index_path.exists() {
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&dist_dir) {
        println!(
            "cargo:warning=failed to create {}: {error}",
            dist_dir.display()
        );
        return;
    }
    let placeholder = "<!doctype html><body style=\"margin:0;background:#171816;color:#e9e9e3;\
         font:14px sans-serif;padding:24px\">base-ui is not built.<br><br>\
         Run <code>pnpm --dir base-ui install && pnpm --dir base-ui build</code>.</body>";
    if let Err(error) = std::fs::write(&index_path, placeholder) {
        println!(
            "cargo:warning=failed to write {}: {error}",
            index_path.display()
        );
    }
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
