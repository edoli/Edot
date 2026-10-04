use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Default, Debug)]
pub struct ScrollRange {
    pub min: i32,
    pub max: i32,
    pub page: u32,
    pub position: i32,
}

impl ScrollRange {
    pub unsafe fn read(editor: HWND, horizontal: bool) -> Self {
        let mut info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            ..Default::default()
        };
        unsafe {
            GetScrollInfo(
                editor,
                if horizontal { SB_HORZ } else { SB_VERT },
                &mut info,
            );
        }
        Self {
            min: info.nMin,
            max: info.nMax,
            page: info.nPage,
            position: info.nPos,
        }
    }

    pub fn extent(self) -> i64 {
        // RichEdit leaves the default 0..100, page=0 range on disabled bars.
        if self.page == 0 {
            return 0;
        }
        (i64::from(self.max) - i64::from(self.min) + 1 - i64::from(self.page.max(1))).max(0)
    }

    pub fn thumb(self, length: i32, minimum: i32) -> (i32, i32) {
        let length = length.max(0);
        let total = (i64::from(self.max) - i64::from(self.min) + 1).max(1);
        let size = (i64::from(length) * i64::from(self.page) / total)
            .clamp(i64::from(minimum.min(length).max(0)), i64::from(length))
            as i32;
        let offset = (i64::from(self.position) - i64::from(self.min)).clamp(0, self.extent());
        let start = (offset * i64::from(length - size) / self.extent().max(1)) as i32;
        (start, size)
    }
}

#[derive(Clone, Copy)]
pub struct ThumbDrag {
    pub horizontal: bool,
    pub range: ScrollRange,
    pub pointer: i32,
    pub length: i32,
    pub minimum: i32,
}

impl ThumbDrag {
    pub fn position(self, pointer: i32) -> i32 {
        let (_, size) = self.range.thumb(self.length, self.minimum);
        let travel = (self.length - size).max(1);
        let delta = (i64::from(pointer) - i64::from(self.pointer)) * self.range.extent()
            / i64::from(travel);
        (i64::from(self.range.position) + delta).clamp(
            i64::from(self.range.min),
            i64::from(self.range.min) + self.range.extent(),
        ) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_keeps_the_grab_position_and_reaches_both_ends() {
        let drag = ThumbDrag {
            horizontal: false,
            range: ScrollRange {
                min: 0,
                max: 2_000_000,
                page: 600,
                position: 123_457,
            },
            pointer: 71,
            length: 500,
            minimum: 24,
        };
        assert_eq!(drag.position(71), 123_457);
        assert!(drag.position(72) > 123_457);
        assert_eq!(drag.position(71), 123_457);
        assert_eq!(drag.position(i32::MIN), 0);
        assert_eq!(i64::from(drag.position(i32::MAX)), drag.range.extent());
    }
    #[test]
    fn thumb_reaches_both_ends_with_large_pixel_ranges() {
        let range = ScrollRange {
            min: 0,
            max: 2_000_000,
            page: 600,
            position: 0,
        };
        assert_eq!(range.thumb(500, 24), (0, 24));
        let drag = ThumbDrag {
            horizontal: false,
            range,
            pointer: 0,
            length: 500,
            minimum: 24,
        };
        let end = drag.position(500);
        assert!(end > 65_535);
        assert_eq!(
            ScrollRange {
                position: end,
                ..range
            }
            .thumb(500, 24),
            (476, 24)
        );
        assert_eq!(drag.position(-5), 0);
        assert_eq!(ThumbDrag { length: 0, ..drag }.position(0), 0);
        assert_eq!(range.thumb(10, 24), (0, 10));
        assert_eq!(
            ScrollRange {
                max: 100,
                ..Default::default()
            }
            .extent(),
            0
        );
        assert_eq!(
            ScrollRange {
                min: 0,
                max: 99,
                page: 100,
                position: 0
            }
            .extent(),
            0
        );
    }
}
