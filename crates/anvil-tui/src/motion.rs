//! Bounded, event-driven transitions for TUI overlays.
//!
//! Motion is a post-render pass over an already rendered buffer. It never feeds
//! back into editor state, cursor placement, scrolling, or LSP updates.

use std::collections::HashSet;

use super::*;
use ratatui::{buffer::Buffer, layout::Rect};

pub(crate) const OVERLAY_FADE: Duration = Duration::from_millis(120);
const NOTIFICATION_FADE: Duration = Duration::from_millis(140);
const PROGRESS_FADE: Duration = Duration::from_millis(160);
const MIN_ALPHA: f32 = 0.15;

#[derive(Default)]
pub(crate) struct MotionState {
    enabled: bool,
    overlay: Option<OverlayFade>,
    alert: Option<AlertFade>,
    notification_generation: Option<u64>,
    seen_progress: HashSet<u64>,
    progress_initialized: bool,
}

struct OverlayFade {
    kind: OverlayKind,
    started: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlertTarget {
    Notifications,
    Progress,
}

struct AlertFade {
    target: AlertTarget,
    started: Instant,
    duration: Duration,
}

impl MotionState {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.overlay = None;
            self.alert = None;
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
        self.overlay.is_some() || self.alert.is_some()
    }

    pub(crate) fn observe_notifications(
        &mut self,
        generation: u64,
        now: Instant,
        reduced_motion: bool,
    ) {
        let changed = self
            .notification_generation
            .is_some_and(|previous| previous != generation);
        self.notification_generation = Some(generation);
        if changed && self.enabled && !reduced_motion {
            self.alert = Some(AlertFade {
                target: AlertTarget::Notifications,
                started: now,
                duration: NOTIFICATION_FADE,
            });
        }
    }

    pub(crate) fn observe_progress(
        &mut self,
        ids: impl IntoIterator<Item = u64>,
        now: Instant,
        reduced_motion: bool,
    ) {
        let current: HashSet<u64> = ids.into_iter().collect();
        if !self.progress_initialized {
            self.seen_progress = current;
            self.progress_initialized = true;
            return;
        }
        let added = current.iter().any(|id| !self.seen_progress.contains(id));
        self.seen_progress = current;
        if added && self.enabled && !reduced_motion {
            self.alert = Some(AlertFade {
                target: AlertTarget::Progress,
                started: now,
                duration: PROGRESS_FADE,
            });
        }
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

    pub(crate) fn fade_notifications(
        &mut self,
        now: Instant,
        rect: Option<Rect>,
        reduced_motion: bool,
    ) -> Option<(Rect, f32)> {
        self.fade_alert(AlertTarget::Notifications, now, rect, reduced_motion)
    }

    pub(crate) fn fade_progress(
        &mut self,
        now: Instant,
        rect: Option<Rect>,
        reduced_motion: bool,
    ) -> Option<(Rect, f32)> {
        self.fade_alert(AlertTarget::Progress, now, rect, reduced_motion)
    }

    fn fade_alert(
        &mut self,
        target: AlertTarget,
        now: Instant,
        rect: Option<Rect>,
        reduced_motion: bool,
    ) -> Option<(Rect, f32)> {
        let alert = self.alert.as_ref()?;
        if alert.target != target {
            return None;
        }
        if reduced_motion {
            self.alert = None;
            return None;
        }
        let Some(rect) = rect else {
            self.alert = None;
            return None;
        };
        let elapsed = now.saturating_duration_since(alert.started);
        if elapsed >= alert.duration {
            self.alert = None;
            return None;
        }
        let progress = elapsed.as_secs_f32() / alert.duration.as_secs_f32();
        let eased = 1.0 - (1.0 - progress).powi(3);
        Some((rect, MIN_ALPHA + (1.0 - MIN_ALPHA) * eased))
    }

    pub(crate) fn clear(&mut self) {
        self.overlay = None;
        self.alert = None;
    }

    #[cfg(test)]
    pub(crate) fn finish(&mut self) {
        self.clear();
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

/// Whether an overlay has a bounded post-render fade target.
pub(crate) fn supports(kind: OverlayKind) -> bool {
    matches!(
        kind,
        OverlayKind::CommandPalette
            | OverlayKind::Fuzzy
            | OverlayKind::BufferPicker
            | OverlayKind::Settings
            | OverlayKind::ThemeBrowser
            | OverlayKind::Problems
            | OverlayKind::DiagnosticPeek
            | OverlayKind::DebugPanel
            | OverlayKind::TestExplorer
            | OverlayKind::CodeActions
            | OverlayKind::DocumentSymbols
            | OverlayKind::References
            | OverlayKind::CallHierarchy
    )
}

/// The post-render area for an animated overlay.
pub(crate) fn overlay_rect(model: &Model, kind: OverlayKind, area: Rect) -> Option<Rect> {
    match kind {
        OverlayKind::CommandPalette => Some(crate::render::command_palette_rect(area)),
        OverlayKind::Fuzzy | OverlayKind::BufferPicker => Some(crate::render::fuzzy_rect(area)),
        OverlayKind::Settings => Some(crate::settings_ui::rect(area)),
        OverlayKind::ThemeBrowser => Some(crate::theme_browser::rect(area)),
        OverlayKind::Problems => Some(crate::render::problems_popup_rect(model, area)),
        OverlayKind::DiagnosticPeek => {
            let peek = model.ui.diagnostic_peek.as_ref()?;
            let cursor_line = model
                .session
                .editor
                .index()
                .position(model.session.editor.text(), model.session.cursor)
                .map(|position| position.line as u16)
                .unwrap_or(0);
            let cursor_y = area
                .y
                .saturating_add(cursor_line.saturating_sub(model.view.scroll as u16))
                .min(area.y + area.height.saturating_sub(1));
            let width = (area.width * 2 / 3)
                .max(30)
                .min(area.width.saturating_sub(2));
            let height = (peek.line_count as u16 + 2)
                .min(12)
                .min(area.height.saturating_sub(2));
            crate::render::diagnostic_peek_rect(area, cursor_y, width, height)
        }
        OverlayKind::DebugPanel => model
            .overlays
            .debug_panel
            .as_ref()
            .map(|panel| crate::debug_panel::popup_rect(area, panel.selected_len())),
        OverlayKind::TestExplorer => {
            model.overlays.test_explorer.as_ref().map(|explorer| {
                crate::test_explorer::popup_rect_for_hit(area, explorer.items.len())
            })
        }
        OverlayKind::CodeActions => model
            .overlays
            .lsp_results
            .code_actions
            .as_ref()
            .map(|actions| crate::render::code_actions_rect(area, actions.items.len())),
        OverlayKind::DocumentSymbols => model
            .overlays
            .lsp_results
            .document_symbols
            .as_ref()
            .map(|symbols| crate::render::document_symbols_rect(area, symbols.items.len())),
        OverlayKind::References => model
            .overlays
            .lsp_results
            .references
            .as_ref()
            .map(|references| crate::render::document_symbols_rect(area, references.items.len())),
        OverlayKind::CallHierarchy => model
            .overlays
            .lsp_results
            .call_hierarchy
            .as_ref()
            .map(|calls| crate::render::document_symbols_rect(area, calls.items.len())),
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

    #[test]
    fn notification_generation_starts_and_cancels_alert_fade() {
        let mut motion = MotionState::default();
        motion.set_enabled(true);
        let now = Instant::now();
        motion.observe_notifications(1, now, false);
        assert!(!motion.active());

        motion.observe_notifications(2, now, false);
        assert!(motion.active());
        let rect = Rect::new(0, 1, 20, 2);
        assert!(motion.fade_notifications(now, Some(rect), false).is_some());
        assert!(motion.active());
        assert!(
            motion
                .fade_notifications(now + NOTIFICATION_FADE, Some(rect), false)
                .is_none()
        );
        assert!(!motion.active());
    }

    #[test]
    fn progress_ids_start_only_for_new_items() {
        let mut motion = MotionState::default();
        motion.set_enabled(true);
        let now = Instant::now();
        motion.observe_progress([1], now, false);
        assert!(!motion.active());
        motion.observe_progress([1], now, false);
        assert!(!motion.active());
        motion.observe_progress([1, 2], now, false);
        assert!(motion.active());
        let rect = Rect::new(0, 1, 20, 3);
        assert!(motion.fade_progress(now, Some(rect), false).is_some());
    }
}
