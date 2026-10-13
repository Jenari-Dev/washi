// SPDX-License-Identifier: GPL-2.0-or-later
//! Text that follows the ring of a disc label ("Follow the ring").
//!
//! The letters sit on a circle about the disc centre, one after the other. The text starts at an
//! angle, measured in degrees clockwise from the top of the disc (12 o'clock = 0, 3 o'clock = 90,
//! the bottom = 180, 9 o'clock = 270), and runs clockwise or counter-clockwise.
//!
//! * **Clockwise**: the letters stand on the circle with their tops pointing outwards, like a title
//!   along the top edge, read from left to right.
//! * **Counter-clockwise**: the letters hang on the inside of the circle with their tops pointing
//!   to the centre, like a line along the bottom edge, also read from left to right.
//!
//! `radius_mm` is the radius of the baseline in both cases. The text `align` of the item says
//! what the start angle means: the text begins there (`left`), is centred on it (`center`) or ends
//! there (`right`). Letter spacing is extra room after every letter, in points; kerning pairs are
//! not applied.
//!
//! The geometry here is a pure function of the letters' advances, so the app's canvas preview
//! (`DiscMath.ringGlyphs` in `qml/DiscMath.qml`) can use the very same formulas, and the tests of
//! both check the same numbers.

use serde::{Deserialize, Serialize};

use super::Align;
use crate::error::{Error, Result};

/// The smallest and largest baseline radius, in mm. The disc itself is 60 mm in radius.
pub const RADIUS_RANGE_MM: (f64, f64) = (5.0, 60.0);
/// Extra letter spacing, in points (negative pulls the letters together).
pub const SPACING_RANGE_PT: (f64, f64) = (-10.0, 100.0);
pub const DEFAULT_RADIUS_MM: f64 = 48.0;

fn default_radius() -> f64 {
    DEFAULT_RADIUS_MM
}

fn default_clockwise() -> bool {
    true
}

/// Where and how a text follows the ring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RingText {
    /// Radius of the baseline, from the disc centre.
    #[serde(default = "default_radius")]
    pub radius_mm: f64,
    /// Degrees clockwise from the top of the disc where the text starts (see [`Align`]).
    #[serde(default)]
    pub start_deg: f64,
    /// The text runs clockwise (letter tops outwards) or counter-clockwise (tops inwards).
    #[serde(default = "default_clockwise")]
    pub clockwise: bool,
    /// Extra space after every letter, in points.
    #[serde(default)]
    pub letter_spacing_pt: f64,
}

impl Default for RingText {
    fn default() -> Self {
        RingText {
            radius_mm: DEFAULT_RADIUS_MM,
            start_deg: 0.0,
            clockwise: true,
            letter_spacing_pt: 0.0,
        }
    }
}

/// One letter placed on the ring: its baseline centre (mm from the disc centre, right and down)
/// and how far to turn it clockwise so that its top points the right way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingGlyph {
    pub x_mm: f64,
    pub y_mm: f64,
    /// Where on the ring (degrees clockwise from the top, not normalised).
    pub angle_deg: f64,
    /// Clockwise rotation of the letter, in (-180, 180].
    pub rotation_deg: f64,
}

/// An angle in degrees brought into (-180, 180].
pub fn normalize_deg(mut a: f64) -> f64 {
    a %= 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    a
}

impl RingText {
    pub fn validate(&self) -> Result<()> {
        let (r0, r1) = RADIUS_RANGE_MM;
        if !(self.radius_mm.is_finite() && (r0..=r1).contains(&self.radius_mm)) {
            return Err(Error::Invalid(format!(
                "ring text radius must be {r0}..{r1} mm, got {}",
                self.radius_mm
            )));
        }
        if !self.start_deg.is_finite() {
            return Err(Error::Invalid("ring text start angle must be a number".into()));
        }
        let (s0, s1) = SPACING_RANGE_PT;
        if !(self.letter_spacing_pt.is_finite() && (s0..=s1).contains(&self.letter_spacing_pt)) {
            return Err(Error::Invalid(format!(
                "ring text letter spacing must be {s0}..{s1} pt, got {}",
                self.letter_spacing_pt
            )));
        }
        Ok(())
    }

    /// Extra space after each letter, in mm.
    pub fn spacing_mm(&self) -> f64 {
        self.letter_spacing_pt * 25.4 / 72.0
    }

    /// The length of the text along its baseline, in mm, for letters with these advances.
    pub fn arc_length_mm(&self, advances_mm: &[f64]) -> f64 {
        let n = advances_mm.len();
        advances_mm.iter().sum::<f64>() + self.spacing_mm() * n.saturating_sub(1) as f64
    }

    /// Place letters whose advance widths (mm, at the text's size) are given.
    pub fn layout(&self, advances_mm: &[f64], align: Align) -> Vec<RingGlyph> {
        let total = self.arc_length_mm(advances_mm);
        let shift = match align {
            Align::Left => 0.0,
            Align::Center => -total / 2.0,
            Align::Right => -total,
        };
        let dir = if self.clockwise { 1.0 } else { -1.0 };
        let r = self.radius_mm.max(1e-6);
        let mut out = Vec::with_capacity(advances_mm.len());
        let mut s = 0.0;
        for adv in advances_mm {
            // The letter's centre lies half its advance past where it begins.
            let along = s + adv / 2.0 + shift;
            let angle = self.start_deg + dir * (along / r).to_degrees();
            let (sin, cos) = angle.to_radians().sin_cos();
            out.push(RingGlyph {
                x_mm: r * sin,
                y_mm: -r * cos,
                angle_deg: angle,
                rotation_deg: normalize_deg(if self.clockwise { angle } else { angle + 180.0 }),
            });
            s += adv + self.spacing_mm();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn at(r: f64, start: f64, clockwise: bool) -> RingText {
        RingText { radius_mm: r, start_deg: start, clockwise, letter_spacing_pt: 0.0 }
    }

    #[test]
    fn a_letter_at_the_top_stands_upright_and_one_at_the_right_leans_over() {
        let g = at(50.0, 0.0, true).layout(&[0.0], Align::Left)[0];
        assert!(close(g.x_mm, 0.0) && close(g.y_mm, -50.0), "{g:?}");
        assert!(close(g.rotation_deg, 0.0));
        let g = at(50.0, 90.0, true).layout(&[0.0], Align::Left)[0];
        assert!(close(g.x_mm, 50.0) && close(g.y_mm, 0.0), "{g:?}");
        assert!(close(g.rotation_deg, 90.0), "tops point outwards, to the right");
        let g = at(50.0, 270.0, true).layout(&[0.0], Align::Left)[0];
        assert!(close(g.x_mm, -50.0) && close(g.rotation_deg, -90.0), "{g:?}");
    }

    #[test]
    fn counter_clockwise_text_at_the_bottom_reads_upright_with_tops_inwards() {
        let g = at(50.0, 180.0, false).layout(&[0.0], Align::Left)[0];
        assert!(close(g.x_mm, 0.0) && close(g.y_mm, 50.0), "{g:?}");
        assert!(close(g.rotation_deg, 0.0), "upright: {g:?}");
        // To the right of the bottom (3 o'clock side): tops lean towards the centre.
        let g = at(50.0, 90.0, false).layout(&[0.0], Align::Left)[0];
        assert!(close(g.x_mm, 50.0) && close(g.rotation_deg, -90.0), "{g:?}");
    }

    #[test]
    fn letters_follow_each_other_along_the_arc_in_the_chosen_direction() {
        let adv = [5.0, 5.0, 5.0];
        let cw = at(50.0, 0.0, true).layout(&adv, Align::Left);
        let step = (5.0f64 / 50.0).to_degrees();
        for (i, g) in cw.iter().enumerate() {
            assert!(close(g.angle_deg, step * (i as f64 + 0.5)), "{i}: {g:?}");
        }
        let ccw = at(50.0, 0.0, false).layout(&adv, Align::Left);
        for (i, g) in ccw.iter().enumerate() {
            assert!(close(g.angle_deg, -step * (i as f64 + 0.5)), "{i}: {g:?}");
        }
    }

    #[test]
    fn every_letter_sits_on_the_circle() {
        for (clockwise, align) in
            [(true, Align::Left), (false, Align::Center), (true, Align::Right)]
        {
            let t =
                RingText { radius_mm: 37.5, start_deg: 123.0, clockwise, letter_spacing_pt: 3.0 };
            for g in t.layout(&[3.0, 4.5, 2.0, 6.0, 5.5], align) {
                assert!(close(g.x_mm.hypot(g.y_mm), 37.5), "{g:?}");
                assert!(g.rotation_deg > -180.0 && g.rotation_deg <= 180.0);
            }
        }
    }

    #[test]
    fn the_alignment_says_what_the_start_angle_means() {
        let adv = [6.0, 6.0];
        let t = at(40.0, 30.0, true);
        let left = t.layout(&adv, Align::Left);
        let centre = t.layout(&adv, Align::Center);
        let right = t.layout(&adv, Align::Right);
        let half = (6.0f64 / 40.0).to_degrees(); // half of the text (12 mm) as an angle
        // Left: the text begins at 30 degrees; its first letter centre is half an advance later.
        assert!(close(left[0].angle_deg, 30.0 + half / 2.0));
        // Centre: the middle of the text, between the two letters, is at 30 degrees.
        assert!(close((centre[0].angle_deg + centre[1].angle_deg) / 2.0, 30.0));
        // Right: the text ends at 30 degrees.
        assert!(close(right[1].angle_deg + half / 2.0, 30.0));
        // Counter-clockwise mirrors it.
        let t = at(40.0, 30.0, false);
        let right = t.layout(&adv, Align::Right);
        assert!(close(right[1].angle_deg - half / 2.0, 30.0));
    }

    #[test]
    fn letter_spacing_adds_room_between_letters_only() {
        let adv = [4.0, 4.0, 4.0];
        let mut t = at(50.0, 0.0, true);
        let plain = t.layout(&adv, Align::Left);
        t.letter_spacing_pt = 72.0; // one inch = 25.4 mm
        let spaced = t.layout(&adv, Align::Left);
        let to_deg = |mm: f64| (mm / 50.0).to_degrees();
        assert!(close(spaced[0].angle_deg, plain[0].angle_deg), "nothing before the first letter");
        assert!(close(spaced[1].angle_deg - plain[1].angle_deg, to_deg(25.4)));
        assert!(close(spaced[2].angle_deg - plain[2].angle_deg, to_deg(50.8)));
        assert!(
            close(t.arc_length_mm(&adv), 12.0 + 2.0 * 25.4),
            "the last letter has no trailing room"
        );
        // Negative spacing pulls the letters together.
        t.letter_spacing_pt = -2.0;
        assert!(t.layout(&adv, Align::Left)[2].angle_deg < plain[2].angle_deg);
    }

    #[test]
    fn nothing_to_place_is_an_empty_layout() {
        assert!(RingText::default().layout(&[], Align::Center).is_empty());
        assert_eq!(RingText::default().arc_length_mm(&[]), 0.0);
    }

    #[test]
    fn angles_are_normalized() {
        assert_eq!(normalize_deg(190.0), -170.0);
        assert_eq!(normalize_deg(-190.0), 170.0);
        assert_eq!(normalize_deg(180.0), 180.0);
        assert_eq!(normalize_deg(-180.0), 180.0);
        assert_eq!(normalize_deg(720.0), 0.0);
    }

    #[test]
    fn the_limits_are_checked() {
        assert!(RingText::default().validate().is_ok());
        let bad = |f: fn(&mut RingText)| {
            let mut t = RingText::default();
            f(&mut t);
            t.validate().is_err()
        };
        assert!(bad(|t| t.radius_mm = 4.0));
        assert!(bad(|t| t.radius_mm = 61.0));
        assert!(bad(|t| t.radius_mm = f64::NAN));
        assert!(bad(|t| t.start_deg = f64::INFINITY));
        assert!(bad(|t| t.letter_spacing_pt = 101.0));
        assert!(bad(|t| t.letter_spacing_pt = -11.0));
    }

    #[test]
    fn json_fills_in_the_defaults() {
        let t: RingText = serde_json::from_str("{}").unwrap();
        assert_eq!(t, RingText::default());
        let t: RingText =
            serde_json::from_str(r#"{"radius_mm": 30, "clockwise": false, "start_deg": 180}"#)
                .unwrap();
        assert_eq!((t.radius_mm, t.start_deg, t.clockwise), (30.0, 180.0, false));
    }
}
