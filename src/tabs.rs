/// Tab coordinates use device pixels. Scroll only after the minimum width is reached.
#[derive(Clone, Copy, Debug, Default)]
pub struct TabLayout {
    pub width: i32,
    pub viewport: i32,
    pub count: usize,
    pub offset: i32,
    pub overflow: bool,
}

impl TabLayout {
    pub fn new(viewport: i32, count: usize, min: i32, preferred: i32, offset: i32) -> Self {
        let viewport = viewport.max(0);
        let width = if count == 0 {
            preferred
        } else {
            (viewport / count as i32).clamp(min, preferred)
        };
        let total = width.saturating_mul(count as i32);
        let max_scroll = (total - viewport).max(0);
        Self {
            width,
            viewport,
            count,
            offset: offset.clamp(0, max_scroll),
            overflow: max_scroll > 0,
        }
    }
    pub fn bounds(&self, index: usize) -> (i32, i32) {
        let left = index as i32 * self.width - self.offset;
        (left, left + self.width)
    }
    pub fn hit(&self, x: i32) -> Option<usize> {
        if x < 0 || x >= self.viewport || self.width == 0 {
            return None;
        }
        let index = ((x + self.offset) / self.width) as usize;
        (index < self.count).then_some(index)
    }
    pub fn reveal(&mut self, index: usize) {
        if index >= self.count {
            return;
        }
        let (left, right) = self.bounds(index);
        if left < 0 {
            self.offset += left;
        } else if right > self.viewport {
            self.offset += right - self.viewport;
        }
        self.offset = self
            .offset
            .clamp(0, (self.count as i32 * self.width - self.viewport).max(0));
    }
    pub fn scroll(&mut self, pixels: i32) {
        self.offset = (self.offset + pixels)
            .clamp(0, (self.count as i32 * self.width - self.viewport).max(0));
    }

    /// Insertion gap in the original order, before removing the dragged tab.
    pub fn insertion(&self, x: i32) -> usize {
        if self.width <= 0 {
            return 0;
        }
        ((x.saturating_add(self.offset).saturating_add(self.width / 2)).max(0) / self.width)
            .min(self.count as i32) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs_shrink_before_scrolling_and_keep_close_space() {
        let normal = TabLayout::new(800, 3, 88, 180, 0);
        assert_eq!(normal.width, 180);
        let compact = TabLayout::new(800, 8, 88, 180, 0);
        assert_eq!(compact.width, 100);
        assert!(!compact.overflow);
        assert_eq!(compact.bounds(7), (700, 800));
        let mut overflow = TabLayout::new(800, 20, 88, 180, 0);
        assert!(overflow.overflow);
        assert_eq!(overflow.width, 88);
        overflow.reveal(19);
        assert_eq!(overflow.bounds(19), (712, 800));
        assert_eq!(overflow.hit(799), Some(19));
        assert_eq!(overflow.hit(800), None);
        overflow.reveal(0);
        assert_eq!(overflow.offset, 0);
        overflow.scroll(i32::MAX / 2);
        assert_eq!(overflow.offset, 960);
    }

    #[test]
    fn insertion_uses_tab_midpoints_and_the_scroll_offset() {
        let layout = TabLayout::new(300, 6, 100, 100, 200);
        assert_eq!(layout.insertion(0), 2);
        assert_eq!(layout.insertion(49), 2);
        assert_eq!(layout.insertion(50), 3);
        assert_eq!(layout.insertion(300), 5);
        assert_eq!(layout.insertion(i32::MAX), 6);
        assert_eq!(layout.insertion(i32::MIN), 0);
    }
}
