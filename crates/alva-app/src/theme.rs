// INPUT:  gpui (Global, Rgba, Window, WindowAppearance, rgb), crate::models::ThemeMode
// OUTPUT: pub struct ActiveThemeMode, pub struct Theme
// POS:    Provides light/dark/system theme resolution and semantic color tokens for the entire UI.
//! Application theme — maps semantic colors to GPUI primitives.

use gpui::{Global, Rgba, Window, WindowAppearance, rgb};

use crate::models::ThemeMode;

/// GPUI Global that stores the user-chosen ThemeMode.
/// All views read this to decide light/dark without needing SettingsModel.
pub struct ActiveThemeMode(pub ThemeMode);

impl Global for ActiveThemeMode {}

#[derive(Clone, Debug)]
pub struct Theme {
    // Base
    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_subtle: Rgba,
    pub background: Rgba,
    pub surface: Rgba,
    pub surface_hover: Rgba,
    pub border: Rgba,
    pub border_subtle: Rgba,

    // Sidebar
    pub sidebar_bg: Rgba,

    // Accent
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub accent_subtle: Rgba,
    pub selected_text: Rgba,

    // Card
    pub card_bg: Rgba,
    pub card_border: Rgba,

    // Semantic
    pub error: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub info: Rgba,
}

impl Theme {
    /// Resolve the theme using the user-chosen ThemeMode (read from GPUI global)
    /// and the system window appearance as fallback when mode == System.
    pub fn for_appearance(window: &Window, cx: &impl std::ops::Deref<Target = gpui::App>) -> Self {
        let mode = cx
            .try_global::<ActiveThemeMode>()
            .map(|g| g.0)
            .unwrap_or(ThemeMode::System);
        Self::for_mode(mode, window)
    }

    /// Resolve the theme for a specific ThemeMode.
    pub fn for_mode(mode: ThemeMode, window: &Window) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
            ThemeMode::System => match window.appearance() {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            },
        }
    }

    pub fn dark() -> Self {
        Self {
            text: rgb(0xE5E7EB),
            text_muted: rgb(0x9CA3AF),
            text_subtle: rgb(0x6B7280),
            background: rgb(0x0F1117),
            surface: rgb(0x1A1D27),
            surface_hover: rgb(0x252833),
            border: rgb(0x2A2D3A),
            border_subtle: rgb(0x1F2230),
            sidebar_bg: rgb(0x141620),
            accent: rgb(0x6366F1),
            accent_hover: rgb(0x4F46E5),
            accent_subtle: rgb(0x1E1B4B),
            selected_text: rgb(0xFFFFFF),
            card_bg: rgb(0x1A1D27),
            card_border: rgb(0x2A2D3A),
            error: rgb(0xF87171),
            success: rgb(0x4ADE80),
            warning: rgb(0xFBBF24),
            info: rgb(0x60A5FA),
        }
    }

    pub fn light() -> Self {
        Self {
            text: rgb(0x1A1A2E),
            text_muted: rgb(0x6B7280),
            text_subtle: rgb(0x9CA3AF),
            background: rgb(0xFFFFFF),
            surface: rgb(0xF8F9FC),
            surface_hover: rgb(0xF0F1F5),
            border: rgb(0xE5E7EB),
            border_subtle: rgb(0xF0F1F5),
            sidebar_bg: rgb(0xF3F4F8),
            accent: rgb(0x6366F1),
            accent_hover: rgb(0x4F46E5),
            accent_subtle: rgb(0xEEF2FF),
            selected_text: rgb(0xFFFFFF),
            card_bg: rgb(0xFFFFFF),
            card_border: rgb(0xE5E7EB),
            error: rgb(0xEF4444),
            success: rgb(0x22C55E),
            warning: rgb(0xF59E0B),
            info: rgb(0x6366F1),
        }
    }
}
