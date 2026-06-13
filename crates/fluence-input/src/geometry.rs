// SPDX-License-Identifier: Apache-2.0

//! Hit-testing: which declared target a normalized pointer falls on (SPEC §4.A).

use fluence_protocol::input::{Rect, Target, Viewport};

/// Maps a normalized surface coordinate in `[0, 1]` to a viewport pixel.
fn to_pixels(normalized: f64, extent: u32) -> f64 {
    normalized * f64::from(extent)
}

/// Whether `(px, py)` (viewport pixels) lies within `rect`. Left and top edges
/// are inclusive, right and bottom exclusive, so two abutting targets never
/// both match a point on their shared edge.
fn contains(rect: Rect, px: f64, py: f64) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

/// The first target containing the normalized point `(x, y)` (both in `[0, 1]`,
/// relative to the surface), or `None`.
///
/// Targets are tested in declaration order, so a UI lists the front-most of any
/// overlapping targets first (the composer's targets do not overlap).
#[must_use]
pub fn hit_test(viewport: Viewport, targets: &[Target], x: f64, y: f64) -> Option<&Target> {
    let px = to_pixels(x, viewport.w);
    let py = to_pixels(y, viewport.h);
    targets.iter().find(|target| contains(target.rect, px, py))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluence_protocol::TargetId;
    use fluence_protocol::input::TargetRole;

    fn target(id: &str, rect: [f64; 4]) -> Target {
        Target {
            id: TargetId::from(id),
            rect: Rect {
                x: rect[0],
                y: rect[1],
                w: rect[2],
                h: rect[3],
            },
            role: TargetRole::Key,
            label: None,
            prior: None,
        }
    }

    #[test]
    fn maps_normalized_to_pixels_and_finds_the_target() {
        let viewport = Viewport { w: 1000, h: 1000 };
        let targets = [target("a", [100.0, 100.0, 200.0, 200.0])];
        // (0.2, 0.2) → (200, 200) px, inside [100, 100, 200, 200].
        assert_eq!(
            hit_test(viewport, &targets, 0.2, 0.2).map(|t| t.id.0.as_str()),
            Some("a")
        );
        // (0.05, 0.05) → (50, 50) px, outside.
        assert!(hit_test(viewport, &targets, 0.05, 0.05).is_none());
    }

    #[test]
    fn edges_are_left_top_inclusive_right_bottom_exclusive() {
        let viewport = Viewport { w: 100, h: 100 };
        let targets = [target("a", [10.0, 50.0, 20.0, 30.0])]; // x:10..30, y:50..80
        // Left/top edges included: (0.10, 0.50) → (10, 50) is inside.
        assert!(hit_test(viewport, &targets, 0.10, 0.50).is_some());
        // Right edge excluded: (0.30, 0.50) → (30, 50) is outside.
        assert!(hit_test(viewport, &targets, 0.30, 0.50).is_none());
        // Bottom edge excluded: (0.10, 0.80) → (10, 80) is outside.
        assert!(hit_test(viewport, &targets, 0.10, 0.80).is_none());
        // Just inside the far corner.
        assert!(hit_test(viewport, &targets, 0.299, 0.799).is_some());
    }

    #[test]
    fn returns_first_of_overlapping_targets() {
        let viewport = Viewport { w: 100, h: 100 };
        let targets = [
            target("front", [0.0, 0.0, 50.0, 50.0]),
            target("back", [0.0, 0.0, 50.0, 50.0]),
        ];
        assert_eq!(
            hit_test(viewport, &targets, 0.1, 0.1).map(|t| t.id.0.as_str()),
            Some("front")
        );
    }
}
