//! Bounded, event-driven transitions for TUI overlays.
//!
//! Motion is a post-render pass over an already rendered buffer. It never feeds
//! back into editor state, cursor placement, scrolling, or LSP updates.

use super::*;
use ratatui::{buffer::Buffer, layout::Rect};

pub(crate) const OVERLAY_FADE: Duration = Duration::from_millis(120);
const MIN_ALPHA: f32 = 0.15;

#[derive(Default)]
pub(crate) struct MotionState {
    enabled: bool,
    overlay: Option<OverlayFade>,
}

struct OverlayFade {
    kind: OverlayKind,
    started: Instant,
}

impl MotionState {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.overlay = None;
        }
    }

    pub(crate) fn start(&mut self, kind: OverlayKind, now: Instant, reduced_motion: bool) {
        if !self.enabled || reduced_motion {
            self.overlay = None;
            return;
        }
        self.overlay = Some(OverlayFade { kind, started: now });
    }

    pub(crate) fn active(&self) -> bool {
        self.overlay.is_some()
    }

    /// Returns the current target and opacity, or clears the transition when
    /// it has completed, the overlay changed, or motion is no longer allowed.
    pub(crate) fn fade(
        &mut self,
        now: Instant,
        top: Option<OverlayKind>,
        rect: Option<Rect>,
        reduced_motion: bool,
    ) -> Option<(Rect, f32)> {
        let fade = self.overlay.as_ref()?;
        if reduced_motion || top != Some(fade.kind) {
            self.overlay = None;
            return None;
        }
        let Some(rect) = rect else {
            self.overlay = None;
            return None;
        };
        let elapsed = now.saturating_duration_since(fade.started);
        if elapsed >= OVERLAY_FADE {
            self.overlay = None;
            return None;
        }
        let progress = elapsed.as_secs_f32() / OVERLAY_FADE.as_secs_f32();
        let eased = 1.0 - (1.0 - progress).powi(3);
        Some((rect, MIN_ALPHA + (1.0 - MIN_ALPHA) * eased))
    }

    #[cfg(test)]
    pub(crate) fn finish(&mut self) {
        self.overlay = None;
    }
}

impl Model {
    pub(crate) fn motion_active(&self) -> bool {
        self.motion.borrow().active()
    }

    pub(crate) fn set_motion_enabled(&self, enabled: bool) {
        self.motion.borrow_mut().set_enabled(enabled);
    }
}

/// The post-render area for an animated overlay.
///
/// The first transition covers the command palette. Other overlays remain
/// static until they get the same bounded fade treatment.
pub(crate) fn overlay_rect(kind: OverlayKind, area: Rect) -> Option<Rect> {
    match kind {
        OverlayKind::CommandPalette => Some(crate::render::command_palette_rect(area)),
        _ => None,
    }
}

/// Fades a rendered overlay toward the theme background.
pub(crate) fn fade_buffer(buffer: &mut Buffer, rect: Rect, alpha: f32) {
    let rect = buffer.area.intersection(rect);
    let background = theme::bg();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            cell.fg = theme::blend(cell.fg, background, alpha);
            cell.bg = theme::blend(cell.bg, background, alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn overlay_fade_progresses_and_expires() {
        let mut motion = MotionState::default();
        motion.set_enabled(true);
        let now = Instant::now();
        motion.start(OverlayKind::CommandPalette, now, false);
        assert!(motion.active());

        let rect = Rect::new(1, 2, 30, 10);
        let (_, start) = motion
            .fade(now, Some(OverlayKind::CommandPalette), Some(rect), false)
            .expect("fade starts");
        assert!((start - MIN_ALPHA).abs() < f32::EPSILON);

        let (_, middle) = motion
            .fade(
                now + Duration::from_millis(60),
                Some(OverlayKind::CommandPalette),
                Some(rect),
                false,
            )
            .expect("fade continues");
        assert!(middle > start && middle < 1.0);

        assert!(
            motion
                .fade(
                    now + OVERLAY_FADE,
                    Some(OverlayKind::CommandPalette),
                    Some(rect),
                    false,
                )
                .is_none()
        );
        assert!(!motion.active());
    }

    #[test]
    fn reduced_motion_or_disabled_motion_never_starts() {
        let mut motion = MotionState::default();
        motion.start(OverlayKind::CommandPalette, Instant::now(), false);
        assert!(!motion.active());

        motion.set_enabled(true);
        motion.start(OverlayKind::CommandPalette, Instant::now(), true);
        assert!(!motion.active());
    }

    #[test]
    fn changing_overlay_cancels_the_fade() {
        let mut motion = MotionState::default();
        motion.set_enabled(true);
        let now = Instant::now();
        motion.start(OverlayKind::CommandPalette, now, false);
        let rect = Rect::new(0, 0, 10, 4);
        assert!(
            motion
                .fade(now, Some(OverlayKind::Problems), Some(rect), false)
                .is_none()
        );
        assert!(!motion.active());
    }

    #[test]
    fn fade_buffer_only_touches_the_target_rect() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        let inside = buffer.cell_mut((0, 0)).expect("inside cell");
        inside.fg = Color::Rgb(255, 255, 255);
        inside.bg = Color::Rgb(20, 20, 20);
        let outside = buffer.cell_mut((3, 0)).expect("outside cell");
        outside.fg = Color::Rgb(255, 255, 255);
        outside.bg = Color::Rgb(20, 20, 20);

        fade_buffer(&mut buffer, Rect::new(0, 0, 2, 1), 0.5);

        assert_ne!(buffer[(0, 0)].fg, Color::Rgb(255, 255, 255));
        assert_eq!(buffer[(3, 0)].fg, Color::Rgb(255, 255, 255));
    }
}
