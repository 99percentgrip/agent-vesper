//! Observation contracts (VB-PRD-001 §7.1, BR-09).
//!
//! Semantic state first; images supplement. Every image carries capture
//! identity, geometry, crop, coordinate transform and a capture time, so a
//! click can name the frame it was planned against. A stale, black or
//! protected capture is not a usable observation just because bytes exist.

use serde::{Deserialize, Serialize};

/// Observation identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObservationId(pub String);

/// What kind of surface the observation covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    /// Semantic state only (JSON structure).
    Semantic,
    /// Semantic state plus one or more bounded images.
    SemanticWithImages,
}

/// Logical→physical coordinate transform for an image (§7.1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoordinateTransform {
    /// Physical offset of the captured surface origin in desktop space
    /// (multi-monitor offsets and negative coordinates are representable).
    pub origin_x: f64,
    pub origin_y: f64,
    /// Display scale factor (fractional scaling representable).
    pub scale: f64,
    /// Rotation of the captured content in degrees (0/90/180/270).
    pub rotation_deg: u16,
}

impl CoordinateTransform {
    /// Map a logical point on the observed surface to physical desktop
    /// coordinates under this transform.
    #[must_use]
    pub fn to_physical(self, logical_x: f64, logical_y: f64) -> (f64, f64) {
        let scaled_x = logical_x * self.scale;
        let scaled_y = logical_y * self.scale;
        match self.rotation_deg.rem_euclid(360) {
            0 => (self.origin_x + scaled_x, self.origin_y + scaled_y),
            90 => (self.origin_x - scaled_y, self.origin_y + scaled_x),
            180 => (self.origin_x - scaled_x, self.origin_y - scaled_y),
            270 => (self.origin_x + scaled_y, self.origin_y - scaled_x),
            other => (
                self.origin_x + scaled_x,
                self.origin_y + scaled_y + (other as f64) * 0.0,
            ),
        }
    }
}

/// Bounds and budget fields for an image part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageBounds {
    pub width_px: u32,
    pub height_px: u32,
    /// Crop rectangle within the captured surface, in surface coordinates.
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    /// Decoded pixel budget this image consumes (NF-07 accounting).
    pub decoded_pixels: u64,
}

/// Bounded observation package. Semantic state is authoritative; images
/// carry the transform data needed to re-validate plans against drift.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub kind: ObservationKind,
    /// Monotonic freshness revision within the session.
    pub revision: u64,
    /// Milliseconds since session start (host-injected; no clock here).
    pub captured_at_ms: u64,
    /// Semantic application state (untrusted content; §9.4).
    pub semantic: serde_json::Value,
    /// Per-image identity/geometry metadata (parallel to payload references
    /// held by the host, not bytes).
    pub images: Vec<crate::identity::CaptureIdentity>,
    pub transforms: Vec<CoordinateTransform>,
    pub bounds: Vec<ImageBounds>,
    /// True when the capture pipeline reported black/protected/stale content
    /// for any image; such an observation must not ground actions (AT-14).
    pub degraded_capture: bool,
}

impl Observation {
    /// An observation with degraded captures cannot ground visually planned
    /// actions regardless of the bytes existing (BR-09, AT-14).
    #[must_use]
    pub fn can_ground_visual_actions(&self) -> bool {
        !self.degraded_capture && self.kind == ObservationKind::SemanticWithImages
    }

    /// Maximum decoded pixels per observation package (NF-07: 16,777,216).
    pub const MAX_DECODED_PIXELS: u64 = 16_777_216;

    /// Combined decoded-pixel budget check for the package.
    #[must_use]
    pub fn within_pixel_budget(&self) -> bool {
        self.bounds
            .iter()
            .map(|bounds| bounds.decoded_pixels)
            .sum::<u64>()
            <= Self::MAX_DECODED_PIXELS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(degraded: bool, pixels: u64) -> Observation {
        Observation {
            id: ObservationId("obs-1".into()),
            kind: ObservationKind::SemanticWithImages,
            revision: 1,
            captured_at_ms: 100,
            semantic: serde_json::json!({"active_project": "Draft"}),
            images: vec![crate::identity::CaptureIdentity {
                node_handle: 3,
                object_serial: Some(9),
                surface_key: "win".into(),
            }],
            transforms: vec![CoordinateTransform {
                origin_x: 0.0,
                origin_y: 0.0,
                scale: 1.0,
                rotation_deg: 0,
            }],
            bounds: vec![ImageBounds {
                width_px: 1920,
                height_px: 1080,
                crop_x: 0,
                crop_y: 0,
                crop_width: 1920,
                crop_height: 1080,
                decoded_pixels: pixels,
            }],
            degraded_capture: degraded,
        }
    }

    #[test]
    fn degraded_captures_cannot_ground_visual_actions() {
        assert!(!observation(true, 1920 * 1080).can_ground_visual_actions());
        assert!(observation(false, 1920 * 1080).can_ground_visual_actions());
        let mut semantic_only = observation(false, 0);
        semantic_only.kind = ObservationKind::Semantic;
        assert!(!semantic_only.can_ground_visual_actions());
    }

    #[test]
    fn pixel_budget_bounds_the_package() {
        assert!(observation(false, 16_777_216).within_pixel_budget());
        assert!(!observation(false, 16_777_217).within_pixel_budget());
    }

    #[test]
    fn transforms_map_multi_monitor_and_rotation_cases() {
        let t = CoordinateTransform {
            origin_x: -1920.0,
            origin_y: 0.0,
            scale: 1.25,
            rotation_deg: 0,
        };
        let (x, y) = t.to_physical(100.0, 40.0);
        assert!((x - (-1920.0 + 125.0)).abs() < 1e-9);
        assert!((y - 50.0).abs() < 1e-9);
        let r90 = CoordinateTransform {
            origin_x: 500.0,
            origin_y: 500.0,
            scale: 1.0,
            rotation_deg: 90,
        };
        let (x, y) = r90.to_physical(10.0, 20.0);
        assert!((x - 480.0).abs() < 1e-9);
        assert!((y - 510.0).abs() < 1e-9);
    }
}
