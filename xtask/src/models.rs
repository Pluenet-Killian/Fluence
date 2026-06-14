// SPDX-License-Identifier: Apache-2.0

//! `models-gc`: reclaim cached model files the manifest no longer references
//! (D-3.2, PLAN 7.4).
//!
//! Planning is the `fluence-models` contract; this command lists the cache,
//! prints the plan, and deletes only with an explicit `--apply` — losing a
//! multi-GB model to an accidental sweep is expensive (Marc's 8 GB machine).

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fluence_models::Manifest;

use crate::assets::models_dir;

/// Exit code for an IO/manifest failure.
const EXIT_FAILURE: u8 = 1;

/// Runs `models-gc`. Dry-run by default; `apply` deletes the unreferenced files.
pub fn gc(repo_root: &Path, apply: bool) -> ExitCode {
    let manifest_path = repo_root.join("models").join("test-assets.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "models-gc: cannot read {}: {error}",
                manifest_path.display()
            );
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let manifest = match Manifest::parse(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("models-gc: invalid manifest: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let dir = models_dir(repo_root);
    let present = match present_model_files(&dir) {
        Ok(present) => present,
        Err(error) => {
            eprintln!("models-gc: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    let plan = fluence_models::plan_gc(&manifest.referenced_files(), &present);
    println!("models-gc: cache {}", dir.display());
    for file in &plan.keep {
        println!("  keep   {file}");
    }
    for file in &plan.remove {
        println!("  remove {file}");
    }
    if plan.remove.is_empty() {
        println!("models-gc: nothing to reclaim.");
        return ExitCode::SUCCESS;
    }
    if !apply {
        println!(
            "models-gc: {} file(s) would be removed; re-run with --apply to delete.",
            plan.remove.len()
        );
        return ExitCode::SUCCESS;
    }

    let mut failed = false;
    for file in &plan.remove {
        let path = dir.join(file);
        match fs::remove_file(&path) {
            Ok(()) => println!("models-gc: removed {}", path.display()),
            Err(error) => {
                eprintln!("models-gc: cannot remove {}: {error}", path.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::from(EXIT_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

/// Regular files in the cache, excluding in-progress `.part` downloads (which
/// `download-test-assets` owns) — GC must never delete a partial fetch.
fn present_model_files(dir: &Path) -> Result<Vec<String>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read cache {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read cache entry: {e}"))?;
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // Skip in-progress downloads — `download-test-assets` owns `<file>.part`.
        if Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("part"))
        {
            continue;
        }
        files.push(name.to_owned());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_files_exclude_partial_downloads_and_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.gguf"), b"x").expect("a");
        fs::write(dir.path().join("b.onnx"), b"y").expect("b");
        fs::write(dir.path().join("c.gguf.part"), b"z").expect("part");
        fs::create_dir(dir.path().join("subdir")).expect("subdir");

        let mut present = present_model_files(dir.path()).expect("list");
        present.sort();
        assert_eq!(present, vec!["a.gguf", "b.onnx"], "no .part, no dirs");
    }

    #[test]
    fn an_absent_cache_is_empty_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        assert!(present_model_files(&missing).expect("ok").is_empty());
    }
}
