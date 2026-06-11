//! Mermaid プレビュー状態の session 実装。

use super::*;

impl EditorSessionState {
    pub fn mermaid_preview_auto(&self) -> bool {
        self.mermaid_preview_auto
    }

    pub fn mermaid_preview_background(&self) -> &str {
        &self.mermaid_preview_background
    }

    pub fn mermaid_preview_width_percent(&self) -> u16 {
        self.mermaid_preview_width_percent
    }

    pub fn mermaid_preview_height_percent(&self) -> u16 {
        self.mermaid_preview_height_percent
    }

    pub fn request_mermaid_preview(&mut self) {
        log::debug!("[editor_session][mermaid_preview] manual preview requested");
        self.mermaid_preview_manual_active = true;
        self.mermaid_preview_view.focused = true;
        self.mermaid_preview_closed = false;
    }

    pub fn mermaid_preview_manual_active(&self) -> bool {
        self.mermaid_preview_manual_active
    }

    pub fn mermaid_preview_focused(&self) -> bool {
        self.mermaid_preview_view.focused
    }

    pub fn mermaid_preview_view(&self) -> MermaidPreviewViewState {
        self.mermaid_preview_view
    }

    pub fn mermaid_preview_closed(&self) -> bool {
        self.mermaid_preview_closed
    }

    pub fn mermaid_preview_zoom(&self) -> MermaidPreviewZoom {
        self.mermaid_preview_view.zoom
    }

    pub fn mermaid_preview_pan(&self) -> (u32, u32) {
        (
            self.mermaid_preview_view.pan_x_px,
            self.mermaid_preview_view.pan_y_px,
        )
    }

    pub fn focus_mermaid_preview(&mut self) {
        if self.mermaid_preview_manual_active || self.mermaid_preview_auto {
            self.mermaid_preview_view.focused = true;
            log::debug!("[editor_session][mermaid_preview] preview focused");
        }
    }

    pub fn unfocus_mermaid_preview(&mut self, reason: &str) {
        if self.mermaid_preview_view.focused {
            log::debug!(
                "[editor_session][mermaid_preview] preview unfocused: reason={}",
                reason
            );
        }
        self.mermaid_preview_view.focused = false;
    }

    pub fn close_mermaid_preview(&mut self, reason: &str) {
        log::debug!(
            "[editor_session][mermaid_preview] preview closed: reason={}",
            reason
        );
        self.mermaid_preview_manual_active = false;
        self.mermaid_preview_view.focused = false;
        self.mermaid_preview_view.zoom = MermaidPreviewZoom::Fit;
        self.mermaid_preview_view.pan_x_px = 0;
        self.mermaid_preview_view.pan_y_px = 0;
        self.mermaid_preview_closed = true;
    }

    pub fn reopen_mermaid_preview_if_closed(&mut self, reason: &str) {
        if self.mermaid_preview_closed {
            log::debug!(
                "[editor_session][mermaid_preview] closed preview suppression cleared: reason={}",
                reason
            );
        }
        self.mermaid_preview_closed = false;
    }

    pub fn zoom_mermaid_preview_in(&mut self) {
        let next = match self.mermaid_preview_view.zoom {
            MermaidPreviewZoom::Fit => 125,
            MermaidPreviewZoom::Percent(percent) => percent.saturating_add(25).min(400),
        };
        self.mermaid_preview_view.zoom = MermaidPreviewZoom::Percent(next);
        log::debug!(
            "[editor_session][mermaid_preview] zoom in: percent={}",
            next
        );
    }

    pub fn zoom_mermaid_preview_out(&mut self) {
        let next = match self.mermaid_preview_view.zoom {
            MermaidPreviewZoom::Fit => 75,
            MermaidPreviewZoom::Percent(percent) => percent.saturating_sub(25).max(25),
        };
        self.mermaid_preview_view.zoom = MermaidPreviewZoom::Percent(next);
        log::debug!(
            "[editor_session][mermaid_preview] zoom out: percent={}",
            next
        );
    }

    pub fn zoom_mermaid_preview_fit(&mut self) {
        self.mermaid_preview_view.zoom = MermaidPreviewZoom::Fit;
        self.mermaid_preview_view.pan_x_px = 0;
        self.mermaid_preview_view.pan_y_px = 0;
        log::debug!("[editor_session][mermaid_preview] zoom reset to fit");
    }

    pub fn zoom_mermaid_preview_actual_size(&mut self) {
        self.mermaid_preview_view.zoom = MermaidPreviewZoom::Percent(100);
        self.mermaid_preview_view.pan_x_px = 0;
        self.mermaid_preview_view.pan_y_px = 0;
        log::debug!("[editor_session][mermaid_preview] zoom set to 100%");
    }

    pub fn pan_mermaid_preview(&mut self, delta_x_px: i32, delta_y_px: i32) {
        self.mermaid_preview_view.pan_x_px =
            apply_signed_delta_u32(self.mermaid_preview_view.pan_x_px, delta_x_px);
        self.mermaid_preview_view.pan_y_px =
            apply_signed_delta_u32(self.mermaid_preview_view.pan_y_px, delta_y_px);
        log::debug!(
            "[editor_session][mermaid_preview] pan updated: x_px={}, y_px={}, delta=({}, {})",
            self.mermaid_preview_view.pan_x_px,
            self.mermaid_preview_view.pan_y_px,
            delta_x_px,
            delta_y_px
        );
    }

    pub fn clear_mermaid_preview_manual(&mut self, reason: &str) {
        if self.mermaid_preview_manual_active {
            log::debug!(
                "[editor_session][mermaid_preview] manual preview cleared: reason={}",
                reason
            );
        }
        self.mermaid_preview_manual_active = false;
        self.unfocus_mermaid_preview(reason);
    }
}
