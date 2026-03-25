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
    pub background: Rgba,
    pub surface: Rgba,
    pub surface_hover: Rgba,
    pub border: Rgba,

    // Accent
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub selected_text: Rgba,

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
            background: rgb(0x111827),
            surface: rgb(0x1F2937),
            surface_hover: rgb(0x374151),
            border: rgb(0x374151),
            accent: rgb(0x3B82F6),
            accent_hover: rgb(0x2563EB),
            selected_text: rgb(0xFFFFFF),
            error: rgb(0xF87171),
            success: rgb(0x4ADE80),
            warning: rgb(0xFBBF24),
            info: rgb(0x60A5FA),
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
            error: rgb(0xEF4444),
            success: rgb(0x22C55E),
            warning: rgb(0xF59E0B),
            info: rgb(0x3B82F6),
        }
    }
}
