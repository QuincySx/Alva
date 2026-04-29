// INPUT:  ratatui::layout::Rect
// OUTPUT: centered_rect, anchored_popup, popup_below
// POS:    Pure-function layout helpers — figure out a popup's Rect given
//         constraints. No state, no rendering. Used by ModalFrame and
//         standalone wherever a popup needs to be placed.

use ratatui::layout::Rect;

/// Centered rectangle of `percent_x` × `percent_y` percent of `area`.
/// Both percents must be in 1..=100. Result is clamped inside `area`.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_h = area.height.saturating_mul(percent_y) / 100;
    let popup_w = area.width.saturating_mul(percent_x) / 100;
    Rect {
        x: area.x + area.width.saturating_sub(popup_w) / 2,
        y: area.y + area.height.saturating_sub(popup_h) / 2,
        width: popup_w,
        height: popup_h,
    }
}

/// Centered rectangle with min/max width and height in absolute rows/cols.
/// Picks `desired` if it fits, otherwise clamps to the available area.
pub fn fixed_centered_rect(desired_w: u16, desired_h: u16, area: Rect) -> Rect {
    let w = desired_w.min(area.width);
    let h = desired_h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// Popup anchored to one edge / corner of `area`. Useful for typeahead
/// menus that hug the bottom-left or status banners at the top-right.
#[derive(Debug, Clone, Copy)]
pub enum Anchor {
    TopLeft, TopRight, BottomLeft, BottomRight,
    Top, Bottom, Left, Right, Center,
}

pub fn anchored_popup(area: Rect, anchor: Anchor, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let (x, y) = match anchor {
        Anchor::TopLeft => (area.x, area.y),
        Anchor::TopRight => (area.x + area.width - w, area.y),
        Anchor::BottomLeft => (area.x, area.y + area.height - h),
        Anchor::BottomRight => (area.x + area.width - w, area.y + area.height - h),
        Anchor::Top => (area.x + (area.width - w) / 2, area.y),
        Anchor::Bottom => (area.x + (area.width - w) / 2, area.y + area.height - h),
        Anchor::Left => (area.x, area.y + (area.height - h) / 2),
        Anchor::Right => (area.x + area.width - w, area.y + (area.height - h) / 2),
        Anchor::Center => (area.x + (area.width - w) / 2, area.y + (area.height - h) / 2),
    };
    Rect { x, y, width: w, height: h }
}

/// Popup that sits just above `anchor_row` (e.g. above the input prompt),
/// hugging the left edge. Width clamped to `max_w` or area width.
pub fn popup_above(anchor_row: u16, area: Rect, max_w: u16, h: u16) -> Rect {
    let w = max_w.min(area.width);
    let y = anchor_row.saturating_sub(h);
    Rect { x: area.x, y, width: w, height: h }
}
