// SPDX-License-Identifier: Apache-2.0

//! `download-gaze-assets`: provision the offline webcam-gaze assets in one
//! command (Phase 6, toward `phase-6-done`).
//!
//! 100% offline (SPEC §1): the `MediaPipe` Face Landmarker model is downloaded
//! once and **sha256-verified** (same integrity contract as the test models),
//! and the WASM runtime — which ships inside the `@mediapipe/tasks-vision` npm
//! package — is copied out of `node_modules`. Both land under
//! `apps/web-client/public/`, where the hub serves them; nothing hits a CDN at
//! runtime. The destinations are the defaults baked into `gaze-source.ts`
//! (`/mediapipe/wasm`, `/models/face_landmarker.task`).

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fluence_models::Manifest;

use crate::assets::{Outcome, ensure_model};

/// Exit code for an IO/manifest/download failure.
const EXIT_FAILURE: u8 = 1;

/// Runs `download-gaze-assets`. With `check_only`, validates the manifest and
/// reports without touching the network or the filesystem.
pub fn run(repo_root: &Path, check_only: bool) -> ExitCode {
    let manifest_path = repo_root.join("models").join("gaze-assets.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "download-gaze-assets: cannot read {}: {error}",
                manifest_path.display()
            );
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let manifest = match Manifest::parse(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("download-gaze-assets: invalid manifest: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    let models_dir = repo_root.join("apps/web-client/public/models");
    let wasm_src = repo_root.join("apps/web-client/node_modules/@mediapipe/tasks-vision/wasm");
    let wasm_dst = repo_root.join("apps/web-client/public/mediapipe/wasm");

    if check_only {
        println!(
            "download-gaze-assets --check: manifest v{} OK, {} model(s); WASM source {}",
            manifest.version,
            manifest.models.len(),
            if wasm_src.exists() {
                "present"
            } else {
                "absent (run `pnpm install`)"
            }
        );
        return ExitCode::SUCCESS;
    }

    if let Err(error) = fs::create_dir_all(&models_dir) {
        eprintln!(
            "download-gaze-assets: cannot create {}: {error}",
            models_dir.display()
        );
        return ExitCode::from(EXIT_FAILURE);
    }
    let agent = ureq::Agent::new_with_defaults();
    for model in &manifest.models {
        match ensure_model(model, &models_dir, &agent) {
            Ok(Outcome::AlreadyValid) => {
                println!("download-gaze-assets: {} — cached, verified", model.id);
            }
            Ok(Outcome::Downloaded) => {
                println!("download-gaze-assets: {} — downloaded, verified", model.id);
            }
            Err(error) => {
                eprintln!("download-gaze-assets: {error}");
                return ExitCode::from(EXIT_FAILURE);
            }
        }
    }

    match copy_wasm(&wasm_src, &wasm_dst) {
        Ok(count) => println!(
            "download-gaze-assets: copied {count} WASM file(s) to {}",
            wasm_dst.display()
        ),
        Err(error) => {
            eprintln!("download-gaze-assets: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    }
    println!("download-gaze-assets: gaze assets ready under apps/web-client/public/");
    ExitCode::SUCCESS
}

/// Copies every file from the `MediaPipe` WASM directory into `dst`. Returns the
/// number copied. Errors (loudly) if the source is missing — that means the npm
/// package is not installed.
fn copy_wasm(src: &Path, dst: &Path) -> Result<usize, String> {
    if !src.exists() {
        return Err(format!(
            "WASM source not found at {} — run `pnpm install` first",
            src.display()
        ));
    }
    fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let mut count = 0;
    for entry in fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("cannot read WASM entry: {e}"))?;
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let to = dst.join(entry.file_name());
        fs::copy(entry.path(), &to).map_err(|e| {
            format!(
                "cannot copy {} → {}: {e}",
                entry.path().display(),
                to.display()
            )
        })?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_wasm_copies_files_and_skips_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("wasm");
        let dst = dir.path().join("public/mediapipe/wasm");
        fs::create_dir_all(&src).expect("src");
        fs::write(src.join("vision_wasm_internal.js"), b"js").expect("js");
        fs::write(src.join("vision_wasm_internal.wasm"), b"wasm").expect("wasm");
        fs::create_dir(src.join("nested")).expect("nested dir");

        let count = copy_wasm(&src, &dst).expect("copy");
        assert_eq!(count, 2, "two files, no dir");
        assert!(dst.join("vision_wasm_internal.wasm").exists());
        assert_eq!(
            fs::read(dst.join("vision_wasm_internal.js")).expect("read"),
            b"js"
        );
    }

    #[test]
    fn copy_wasm_errors_when_source_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = copy_wasm(&dir.path().join("nope"), &dir.path().join("dst")).expect_err("err");
        assert!(err.contains("pnpm install"), "guidance present: {err}");
    }
}
