// SPDX-License-Identifier: Apache-2.0

//! SPDX license-header verification (PLAN Phase 0, task 0.2).
//!
//! Enforces the per-layer licensing decided in SPEC D-10.1: every source
//! file must carry the `SPDX-License-Identifier` of the layer it lives in,
//! and source code may only live inside the designated workspace roots —
//! code anywhere else is an architecture violation, not just a licensing
//! one.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Workspace roots and the SPDX identifier their source files must carry
/// (SPEC D-10.1; `ml/` and `xtask/` rationale in ADR-0003).
const LICENSE_ROOTS: &[(&str, &str)] = &[
    ("crates", "Apache-2.0"),
    ("packages", "Apache-2.0"),
    ("ml", "Apache-2.0"),
    ("xtask", "Apache-2.0"),
    ("apps", "AGPL-3.0-only"),
];

/// File extensions treated as source code.
const SOURCE_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "py"];

/// Directory names never traversed (artifacts, dependencies, VCS internals).
const IGNORED_DIRS: &[&str] = &["target", "node_modules", ".venv", "dist", ".git"];

/// The header must appear within this many leading lines, leaving room for
/// shebangs or encoding cookies above it.
const HEADER_SEARCH_LINES: usize = 5;

/// What the D-10.1 layout requires of one source file.
#[derive(Debug, PartialEq, Eq)]
pub enum Requirement {
    /// The file must carry this SPDX identifier in its first lines.
    Spdx(&'static str),
    /// Source code is not allowed outside the designated roots.
    OutsideRoots,
}

/// A file that breaks the layout, with the reason.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub kind: ViolationKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// Header absent or carrying a different identifier than required.
    MissingHeader { expected: &'static str },
    /// Source file outside crates/, packages/, ml/, xtask/, apps/.
    OutsideRoots,
    /// The file could not be read (permissions, encoding).
    Unreadable { error: String },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match &self.kind {
            ViolationKind::MissingHeader { expected } => write!(
                f,
                "{path}: missing `SPDX-License-Identifier: {expected}` in the first {HEADER_SEARCH_LINES} lines"
            ),
            ViolationKind::OutsideRoots => write!(
                f,
                "{path}: source file outside the designated roots (crates/, packages/, ml/, xtask/, apps/) — see LICENSE.md"
            ),
            ViolationKind::Unreadable { error } => write!(f, "{path}: unreadable ({error})"),
        }
    }
}

/// Returns what D-10.1 requires of the source file at `relative_path`
/// (a path relative to the repository root).
pub fn requirement_for(relative_path: &Path) -> Requirement {
    let first_component = relative_path
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    let Some(first) = first_component else {
        return Requirement::OutsideRoots;
    };
    for (root, license) in LICENSE_ROOTS {
        if first == *root {
            return Requirement::Spdx(license);
        }
    }
    Requirement::OutsideRoots
}

/// Returns `true` when `content` carries the required SPDX identifier within
/// the first [`HEADER_SEARCH_LINES`] lines. Comment style is free (`//`, `#`)
/// — only the identifier text matters.
pub fn has_spdx_header(content: &str, license: &str) -> bool {
    let needle = format!("SPDX-License-Identifier: {license}");
    content
        .lines()
        .take(HEADER_SEARCH_LINES)
        .any(|line| line.contains(&needle))
}

/// Checks every source file under `repo_root` against the D-10.1 layout.
///
/// Returns the number of conforming files, or the complete list of
/// violations (never just the first one — fixing them in one pass matters
/// for CI ergonomics).
pub fn check(repo_root: &Path) -> Result<usize, Vec<Violation>> {
    let mut source_files = Vec::new();
    collect_source_files(repo_root, &mut source_files);
    source_files.sort();

    let mut violations = Vec::new();
    for absolute in &source_files {
        let relative = absolute
            .strip_prefix(repo_root)
            .expect("collect_source_files only yields paths under repo_root");
        match requirement_for(relative) {
            Requirement::OutsideRoots => violations.push(Violation {
                path: relative.to_path_buf(),
                kind: ViolationKind::OutsideRoots,
            }),
            Requirement::Spdx(expected) => match fs::read_to_string(absolute) {
                Ok(content) if has_spdx_header(&content, expected) => {}
                Ok(_) => violations.push(Violation {
                    path: relative.to_path_buf(),
                    kind: ViolationKind::MissingHeader { expected },
                }),
                Err(error) => violations.push(Violation {
                    path: relative.to_path_buf(),
                    kind: ViolationKind::Unreadable {
                        error: error.to_string(),
                    },
                }),
            },
        }
    }

    if violations.is_empty() {
        Ok(source_files.len())
    } else {
        Err(violations)
    }
}

/// Walks `dir` recursively, collecting files whose extension is in
/// [`SOURCE_EXTENSIONS`], skipping [`IGNORED_DIRS`]. I/O errors on traversal
/// are deliberately fatal: a directory we cannot list could hide violations.
fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .and_then(Iterator::collect::<io::Result<Vec<_>>>)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !IGNORED_DIRS.contains(&name.as_str()) {
                collect_source_files(&path, out);
            }
            continue;
        }
        let is_source = path
            .extension()
            .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.to_string_lossy().as_ref()));
        if is_source {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bricks_require_apache() {
        for path in [
            "crates/fluence-protocol/src/lib.rs",
            "packages/sdk/src/index.ts",
            "ml/eval/src/fluence_eval/__init__.py",
            "xtask/src/main.rs",
        ] {
            assert_eq!(
                requirement_for(Path::new(path)),
                Requirement::Spdx("Apache-2.0"),
                "{path}"
            );
        }
    }

    #[test]
    fn applications_require_agpl() {
        assert_eq!(
            requirement_for(Path::new("apps/web-client/src/main.ts")),
            Requirement::Spdx("AGPL-3.0-only")
        );
    }

    #[test]
    fn source_outside_roots_is_a_violation() {
        for path in ["scripts/build.rs", "tool.py", "docs/example.ts"] {
            assert_eq!(
                requirement_for(Path::new(path)),
                Requirement::OutsideRoots,
                "{path}"
            );
        }
    }

    #[test]
    fn header_is_found_anywhere_in_leading_lines() {
        let rust_style = "// SPDX-License-Identifier: Apache-2.0\npub fn f() {}\n";
        assert!(has_spdx_header(rust_style, "Apache-2.0"));

        let after_shebang = "#!/usr/bin/env python\n# SPDX-License-Identifier: Apache-2.0\n";
        assert!(has_spdx_header(after_shebang, "Apache-2.0"));
    }

    #[test]
    fn header_must_match_the_required_license() {
        let agpl_header = "// SPDX-License-Identifier: AGPL-3.0-only\n";
        assert!(!has_spdx_header(agpl_header, "Apache-2.0"));
        assert!(has_spdx_header(agpl_header, "AGPL-3.0-only"));
    }

    #[test]
    fn header_beyond_leading_lines_does_not_count() {
        let buried = "\n\n\n\n\n// SPDX-License-Identifier: Apache-2.0\n";
        assert!(!has_spdx_header(buried, "Apache-2.0"));
    }
}
