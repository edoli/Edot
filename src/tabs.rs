/// Which tabs a close operation removes, relative to the clicked tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseMode {
    Current,
    Others,
    ToRight,
}

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
