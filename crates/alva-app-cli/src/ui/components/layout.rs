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
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
    Center,
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
        Anchor::Center => (
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
        ),
    };
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Popup that sits just above `anchor_row` (e.g. above the input prompt),
/// hugging the left edge. Width clamped to `max_w` or area width.
pub fn popup_above(anchor_row: u16, area: Rect, max_w: u16, h: u16) -> Rect {
    let w = max_w.min(area.width);
    let y = anchor_row.saturating_sub(h);
    Rect {
        x: area.x,
        y,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the pure-Rect layout helpers. Mistakes here surface
    //! as off-center popups or overflows, which are easy to miss in
    //! manual UI checks — pin all four functions + each of the 9
    //! Anchor variants.
    use super::*;

    fn area(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    // -- centered_rect -----------------------------------------------------

    #[test]
    fn centered_rect_fifty_percent_of_100x100_is_50x50_centered() {
        let r = centered_rect(50, 50, area(0, 0, 100, 100));
        assert_eq!(r.width, 50);
        assert_eq!(r.height, 50);
        // (100 - 50) / 2 = 25
        assert_eq!(r.x, 25);
        assert_eq!(r.y, 25);
    }

    #[test]
    fn centered_rect_100_percent_returns_full_area_anchored_at_zero() {
        let r = centered_rect(100, 100, area(0, 0, 80, 24));
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
        // (80 - 80) / 2 = 0
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
    }

    #[test]
    fn centered_rect_zero_percent_returns_zero_size_at_center() {
        // 0% × area is a degenerate 0×0 rect. Don't crash, just
        // produce a zero-sized rect at the area's geometric center.
        let r = centered_rect(0, 0, area(0, 0, 100, 100));
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
        assert_eq!(r.x, 50);
        assert_eq!(r.y, 50);
    }

    #[test]
    fn centered_rect_respects_area_offset() {
        // Area not at (0,0) — output must be offset accordingly.
        let r = centered_rect(50, 50, area(10, 20, 100, 100));
        assert_eq!(r.x, 10 + 25);
        assert_eq!(r.y, 20 + 25);
    }

    // -- fixed_centered_rect -----------------------------------------------

    #[test]
    fn fixed_centered_rect_uses_desired_when_it_fits() {
        let r = fixed_centered_rect(40, 10, area(0, 0, 100, 30));
        assert_eq!(r.width, 40);
        assert_eq!(r.height, 10);
        // (100-40)/2 = 30, (30-10)/2 = 10
        assert_eq!(r.x, 30);
        assert_eq!(r.y, 10);
    }

    #[test]
    fn fixed_centered_rect_clamps_to_area_when_desired_too_big() {
        // desired exceeds available — must clamp to area, resulting in
        // x=area.x and y=area.y (centered offset becomes 0).
        let r = fixed_centered_rect(200, 50, area(5, 7, 80, 24));
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 7);
    }

    // -- anchored_popup: every Anchor variant ------------------------------

    #[test]
    fn anchored_popup_top_left() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::TopLeft, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 20, 10));
    }

    #[test]
    fn anchored_popup_top_right() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::TopRight, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (80, 0, 20, 10));
    }

    #[test]
    fn anchored_popup_bottom_left() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::BottomLeft, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 40, 20, 10));
    }

    #[test]
    fn anchored_popup_bottom_right() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::BottomRight, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (80, 40, 20, 10));
    }

    #[test]
    fn anchored_popup_top_middle_centers_horizontally() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::Top, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (40, 0, 20, 10));
    }

    #[test]
    fn anchored_popup_bottom_middle_centers_horizontally() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::Bottom, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (40, 40, 20, 10));
    }

    #[test]
    fn anchored_popup_left_middle_centers_vertically() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::Left, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 20, 20, 10));
    }

    #[test]
    fn anchored_popup_right_middle_centers_vertically() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::Right, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (80, 20, 20, 10));
    }

    #[test]
    fn anchored_popup_center() {
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::Center, 20, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (40, 20, 20, 10));
    }

    #[test]
    fn anchored_popup_clamps_size_to_area() {
        // Requested 200×100 in a 100×50 area: width and height clamp.
        let r = anchored_popup(area(0, 0, 100, 50), Anchor::TopLeft, 200, 100);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 50);
    }

    // -- popup_above -------------------------------------------------------

    #[test]
    fn popup_above_places_above_anchor_row() {
        // anchor_row=20, h=5 → y = 20 - 5 = 15
        let r = popup_above(20, area(0, 0, 100, 30), 50, 5);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 15);
        assert_eq!(r.width, 50);
        assert_eq!(r.height, 5);
    }

    #[test]
    fn popup_above_clamps_width_to_area_width() {
        // max_w=200 > area.width=80 → width clamps to 80.
        let r = popup_above(10, area(0, 0, 80, 24), 200, 4);
        assert_eq!(r.width, 80);
    }

    #[test]
    fn popup_above_saturating_sub_protects_against_underflow() {
        // anchor_row=3, h=10 → 3.saturating_sub(10) = 0 (NOT panic
        // or wrap). Pinned: this is the only guard against popups
        // rendering off-screen when the input prompt is near the top
        // of a small terminal.
        let r = popup_above(3, area(0, 0, 100, 30), 50, 10);
        assert_eq!(r.y, 0, "saturating_sub must clamp to 0, not panic");
    }

    #[test]
    fn popup_above_respects_area_x_offset() {
        // Non-zero area.x — popup_above should anchor at area.x, not 0.
        let r = popup_above(10, area(5, 0, 80, 24), 50, 4);
        assert_eq!(r.x, 5);
    }
}
