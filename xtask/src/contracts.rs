// SPDX-License-Identifier: Apache-2.0

//! `cargo xtask check-contracts` — generates the contract artifacts and
//! fails when the committed files differ (PLAN task 1.3).
//!
//! Artifacts:
//! - `schemas/<Type>.json` — one self-contained JSON Schema per root type
//!   (the reviewable goldens);
//! - `schemas/openapi.json` — the assembled `OpenAPI` 3.1 document;
//! - `packages/sdk/src/generated/api.d.ts` — TypeScript types, generated
//!   from the `OpenAPI` document via `openapi-typescript` (pnpm).
//!
//! Modes: default **writes** the artifacts (developer flow: regenerate,
//! review the diff, commit); `--check` **compares** without writing and
//! exits non-zero on any drift (CI flow).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Relative path of the generated TypeScript types.
const TS_OUTPUT: &str = "packages/sdk/src/generated/api.d.ts";

/// Runs the contracts pipeline. `check = true` compares instead of writing.
///
/// Generation always goes through `target/contracts-gen/` first; goldens on
/// disk are only ever compared against or overwritten as a final step, so
/// both modes exercise the exact same pipeline.
pub fn run(repo_root: &Path, check: bool) -> ExitCode {
    let mut artifacts: BTreeMap<PathBuf, String> = BTreeMap::new();

    for (name, schema) in fluence_protocol::contracts::standalone_schemas() {
        artifacts.insert(
            repo_root.join("schemas").join(format!("{name}.json")),
            pretty(&schema),
        );
    }
    let openapi = fluence_protocol::contracts::openapi_document();
    artifacts.insert(
        repo_root.join("schemas").join("openapi.json"),
        pretty(&openapi),
    );

    let ts_generated = match generate_typescript(repo_root, &pretty(&openapi)) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("check-contracts: TypeScript generation failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    artifacts.insert(repo_root.join(TS_OUTPUT), ts_generated);

    if check {
        let drifted = diff_against_disk(&artifacts);
        if drifted.is_empty() {
            println!(
                "check-contracts: {} artifacts match the source of truth.",
                artifacts.len()
            );
            ExitCode::SUCCESS
        } else {
            for path in &drifted {
                eprintln!(
                    "{}: drifted from fluence-protocol (regenerate and commit)",
                    path.display()
                );
            }
            eprintln!(
                "\ncheck-contracts: {} artifact(s) drifted. Run `cargo xtask check-contracts` \
                 locally and commit the result.",
                drifted.len()
            );
            ExitCode::FAILURE
        }
    } else {
        if let Err(error) = write_all(&artifacts) {
            eprintln!("check-contracts: {error}");
            return ExitCode::FAILURE;
        }
        println!(
            "check-contracts: wrote {} artifacts (schemas/ + {TS_OUTPUT}).",
            artifacts.len()
        );
        ExitCode::SUCCESS
    }
}

/// Pretty-prints with a trailing newline (matches editor/CI conventions).
fn pretty(value: &serde_json::Value) -> String {
    let mut out = serde_json::to_string_pretty(value).expect("contract values serialize");
    out.push('\n');
    out
}

/// Writes every artifact, creating parent directories.
fn write_all(artifacts: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    for (path, content) in artifacts {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Returns the artifacts whose on-disk content differs (or is missing).
fn diff_against_disk(artifacts: &BTreeMap<PathBuf, String>) -> Vec<PathBuf> {
    artifacts
        .iter()
        .filter(|(path, expected)| fs::read_to_string(path).ok().as_ref() != Some(expected))
        .map(|(path, _)| path.clone())
        .collect()
}

/// Runs `pnpm exec openapi-typescript` against a temporary copy of the
/// `OpenAPI` document and returns the generated content, prefixed with the
/// SPDX header (generated files follow D-10.1 like every other source).
fn generate_typescript(repo_root: &Path, openapi_json: &str) -> Result<String, String> {
    let tmp = repo_root.join("target").join("contracts-gen");
    fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    let input = tmp.join("openapi.json");
    fs::write(&input, openapi_json)
        .map_err(|e| format!("cannot write {}: {e}", input.display()))?;
    let output = tmp.join("api.d.ts");

    let mut command = pnpm_command();
    command
        .args(["exec", "openapi-typescript"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .current_dir(repo_root);
    let status = command
        .status()
        .map_err(|e| format!("cannot run pnpm (is it installed?): {e}"))?;
    if !status.success() {
        return Err(format!("openapi-typescript exited with {status}"));
    }
    let generated = fs::read_to_string(&output)
        .map_err(|e| format!("cannot read {}: {e}", output.display()))?;
    Ok(format!(
        "// SPDX-License-Identifier: Apache-2.0\n{generated}"
    ))
}

/// `pnpm` is a `.cmd` shim on Windows; `CreateProcess` cannot spawn those
/// directly.
fn pnpm_command() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "pnpm"]);
        command
    } else {
        Command::new("pnpm")
    }
}
