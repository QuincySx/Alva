//! Application theme — maps semantic colors to GPUI primitives.

use gpui::{Rgba, Window, WindowAppearance, rgb};

#[derive(Clone, Debug)]
pub struct Theme {
    // Base
    pub text: Rgba,
    pub text_muted: Rgba,
    pub background: Rgba,
    pub surface: Rgba,
    pub surface_hover: Rgba,
    pub border: Rgba,

    // Accent
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub selected_text: Rgba,
}

impl Theme {
    pub fn for_appearance(window: &Window) -> Self {
        match window.appearance() {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    pub fn dark() -> Self {
        Self {
            text: rgb(0xE5E7EB),
            text_muted: rgb(0x9CA3AF),
            background: rgb(0x111827),
            surface: rgb(0x1F2937),
            surface_hover: rgb(0x374151),
            border: rgb(0x374151),
            accent: rgb(0x3B82F6),
            accent_hover: rgb(0x2563EB),
            selected_text: rgb(0xFFFFFF),
        }
    }

    pub fn light() -> Self {
        Self {
            text: rgb(0x1F2937),
            text_muted: rgb(0x6B7280),
            background: rgb(0xFFFFFF),
            surface: rgb(0xF3F4F6),
            surface_hover: rgb(0xE5E7EB),
            border: rgb(0xD1D5DB),
            accent: rgb(0x3B82F6),
            accent_hover: rgb(0x2563EB),
            selected_text: rgb(0xFFFFFF),
        }
    }
}
