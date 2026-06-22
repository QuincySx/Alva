// INPUT:  ratatui (Frame, Rect), ratatui_image (Image, picker::Picker as ImgPicker)
// OUTPUT: ImageView — facade for displaying an image inside a Rect
// POS:    Inline image display in the TUI for tool outputs that produce
//         images (screenshots, diagrams). Uses ratatui-image's protocol
//         negotiation (Sixel / iTerm / Kitty / fallback Halfblock) so the
//         right encoding is picked per terminal.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::Frame;
use ratatui_image::picker::Picker as ImgPicker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

/// Holds a decoded image and its resize-aware protocol state.
pub struct ImageView {
    state: StatefulProtocol,
}

impl ImageView {
    /// Load from disk. Lossy: returns `None` if reading or decoding failed
    /// (we don't propagate Err so callers can degrade gracefully — fall
    /// back to a placeholder text in the message stream).
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let picker = ImgPicker::from_query_stdio().ok()?;
        let img = image::ImageReader::open(path).ok()?.decode().ok()?;
        Some(Self {
            state: picker.new_resize_protocol(img),
        })
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let widget = StatefulImage::default();
        frame.render_stateful_widget(widget, area, &mut self.state);
    }
}
