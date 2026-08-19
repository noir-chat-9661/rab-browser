//! Rebuilds `base-ui/dist/index.html` automatically so a stale chrome UI
//! (missing new features, or worse, subtly broken rendering) never ships
//! just because someone forgot to run `pnpm --dir base-ui build` by hand
//! after pulling changes that touch base-ui.

use std::{env, path::Path, process::Command};

fn env_flag_set(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        )
    })
}

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
    // RAB_REQUIRE_BASE_UI is only read at build time below, so Cargo must be
    // told explicitly to rerun this script when it changes (e.g. re-running
    // a build locally with it newly set, without touching any base-ui file).
    println!("cargo:rerun-if-env-changed=RAB_REQUIRE_BASE_UI");

    // RAB_REQUIRE_BASE_UI is set by CI release builds: a placeholder silently
    // shipping in a release binary is much worse than a build failure, and
    // this exact silent fallback already shipped a broken UI to
    // v0.1.0-beta.1/beta.2's Windows binaries undetected. In strict mode
    // every failure path below is a hard `panic!` instead of a warning.
    let strict = env_flag_set("RAB_REQUIRE_BASE_UI");

    if !base_ui_dir.exists() {
        if strict {
            panic!("base-ui directory not found and RAB_REQUIRE_BASE_UI is set");
        }
        ensure_placeholder(&base_ui_dir);
        return;
    }

    let pnpm_bin = match which_pnpm() {
        Ok(bin) => bin,
        Err(()) => {
            if strict {
                panic!(
                    "pnpm not found on PATH and RAB_REQUIRE_BASE_UI is set; \
                     refusing to ship a placeholder chrome UI"
                );
            }
            println!(
                "cargo:warning=pnpm not found on PATH; skipping automatic base-ui build. \
                 Run `pnpm --dir base-ui install && pnpm --dir base-ui build` manually, \
                 or the chrome UI will be stale/missing."
            );
            ensure_placeholder(&base_ui_dir);
            return;
        }
    };

    // Always run install (not just when node_modules is missing): pnpm skips
    // reinstalling unchanged deps in well under a second, and this build.rs
    // only runs at all when base-ui's watched files changed (rerun-if-changed
    // above), so the cost is paid rarely and buys correctness when
    // package.json/pnpm-lock.yaml changed but node_modules was left stale.
    run(
        pnpm_bin,
        &["install", "--frozen-lockfile"],
        &base_ui_dir,
        "install",
        strict,
    );

    run(pnpm_bin, &["run", "build"], &base_ui_dir, "build", strict);

    let index_path = base_ui_dir.join("dist").join("index.html");
    if strict && !index_path.exists() {
        panic!(
            "{} was not produced by the base-ui build and RAB_REQUIRE_BASE_UI is set",
            index_path.display()
        );
    }

    // include_str!(base-ui/dist/index.html) needs a file to exist at compile
    // time regardless of whether the pnpm build above actually succeeded.
    ensure_placeholder(&base_ui_dir);
}

/// Writes a minimal fallback `dist/index.html` if none exists yet, so
/// `include_str!` in main.rs always has something to embed. Never overwrites
/// a real (or even stale) build output. Panics on failure instead of just
/// warning: silently leaving no file behind would otherwise surface as a
/// confusing `include_str!` "file not found" compile error in main.rs with
/// no indication of the real (permissions/disk) cause.
fn ensure_placeholder(base_ui_dir: &Path) {
    let dist_dir = base_ui_dir.join("dist");
    let index_path = dist_dir.join("index.html");
    if index_path.exists() {
        return;
    }
    std::fs::create_dir_all(&dist_dir)
        .unwrap_or_else(|error| panic!("failed to create {}: {error}", dist_dir.display()));
    let placeholder = "<!doctype html><body style=\"margin:0;background:#171816;color:#e9e9e3;\
         font:14px sans-serif;padding:24px\">base-ui is not built.<br><br>\
         Run <code>pnpm --dir base-ui install && pnpm --dir base-ui build</code>.</body>";
    std::fs::write(&index_path, placeholder)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", index_path.display()));
}

// On Windows, pnpm usually installs as a `pnpm.cmd`/`pnpm.ps1` shim (npm
// global install, Corepack), not `pnpm.exe`. `std::process::Command` invokes
// `CreateProcess` directly and does not do the PATHEXT resolution a shell
// would, so `Command::new("pnpm")` silently fails to find the shim there
// even when it's genuinely on PATH — build.rs would then skip the base-ui
// build without any indication beyond a warning, and the compiled-in
// placeholder HTML ("base-ui is not built") would ship in release binaries.
// Standalone installs (e.g. pnpm.io's self-contained binary) do ship an
// actual pnpm.exe, so try the shim first but fall back to the bare name.
const PNPM_CANDIDATES: &[&str] = if cfg!(windows) {
    &["pnpm.cmd", "pnpm"]
} else {
    &["pnpm"]
};

fn which_pnpm() -> Result<&'static str, ()> {
    PNPM_CANDIDATES
        .iter()
        .copied()
        .find(|bin| {
            Command::new(bin)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .ok_or(())
}

fn run(pnpm_bin: &str, args: &[&str], dir: &Path, label: &str, strict: bool) {
    // build.rs always runs non-interactively (no TTY). Without CI=1, pnpm's
    // "confirm modules purge" prompt has nothing to answer and pnpm aborts
    // instead: https://github.com/pnpm/pnpm/issues (ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY).
    match Command::new(pnpm_bin)
        .args(args)
        .current_dir(dir)
        .env("CI", "1")
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) if strict => {
            panic!("base-ui {label} exited with {status} and RAB_REQUIRE_BASE_UI is set")
        }
        Ok(status) => {
            println!("cargo:warning=base-ui {label} exited with {status}; chrome UI may be stale");
        }
        Err(error) if strict => {
            panic!("failed to run base-ui {label}: {error} (RAB_REQUIRE_BASE_UI is set)")
        }
        Err(error) => {
            println!("cargo:warning=failed to run base-ui {label}: {error}");
        }
    }
}
