// SPDX-License-Identifier: Apache-2.0

//! Model garbage collection (D-3.2): reclaim cached model files the manifest no
//! longer references, so an upgraded or re-pinned model does not leave its
//! predecessor wasting disk forever (Marc's 8 GB machine, SPEC personas).
//!
//! Pure planning here; the caller does the deletion, and only with an explicit
//! opt-in — losing a multi-GB model to an accidental sweep is expensive.

use std::collections::BTreeSet;

/// A garbage-collection plan: which present files the manifest still references
/// (`keep`) and which it does not (`remove`). Both are sorted and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcPlan {
    /// Present files the manifest references — left untouched.
    pub keep: Vec<String>,
    /// Present files no manifest entry references — candidates for deletion.
    pub remove: Vec<String>,
}

/// Plans GC of a model directory: every `present` file the manifest does not
/// reference is a removal candidate. Pure and total — the caller restricts
/// `present` to actual model files and applies the plan explicitly.
#[must_use]
pub fn plan_gc(referenced: &BTreeSet<String>, present: &[String]) -> GcPlan {
    let mut keep: Vec<String> = Vec::new();
    let mut remove: Vec<String> = Vec::new();
    for file in present {
        if referenced.contains(file) {
            keep.push(file.clone());
        } else {
            remove.push(file.clone());
        }
    }
    for list in [&mut keep, &mut remove] {
        list.sort();
        list.dedup();
    }
    GcPlan { keep, remove }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn removes_unreferenced_keeps_referenced() {
        let referenced = refs(&["a.gguf", "b.gguf"]);
        let present = vec![
            "a.gguf".to_owned(),
            "b.gguf".to_owned(),
            "old.gguf".to_owned(),
            "stale.onnx".to_owned(),
        ];
        let plan = plan_gc(&referenced, &present);
        assert_eq!(plan.keep, vec!["a.gguf", "b.gguf"]);
        assert_eq!(plan.remove, vec!["old.gguf", "stale.onnx"]);
    }

    #[test]
    fn a_referenced_file_absent_from_disk_is_not_invented() {
        // The manifest references c.gguf but it is not on disk: GC neither keeps
        // nor removes it (it only ever acts on files that exist).
        let plan = plan_gc(&refs(&["c.gguf"]), &["d.gguf".to_owned()]);
        assert!(plan.keep.is_empty());
        assert_eq!(plan.remove, vec!["d.gguf"]);
    }

    #[test]
    fn empty_directory_is_a_noop() {
        let plan = plan_gc(&refs(&["a.gguf"]), &[]);
        assert!(plan.keep.is_empty() && plan.remove.is_empty());
    }

    #[test]
    fn duplicates_are_collapsed() {
        let plan = plan_gc(
            &refs(&["a"]),
            &["a".to_owned(), "a".to_owned(), "b".to_owned()],
        );
        assert_eq!(plan.keep, vec!["a"]);
        assert_eq!(plan.remove, vec!["b"]);
    }
}
