use crate::*;

impl<'a> Term<'a> {
    /// Default stops every eight columns, which is what a terminal powers on
    /// with and what RIS restores.
    pub(crate) fn reset_tabs(&mut self) {
        for (i, stop) in self.s.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
    }

    /// HT, and the engine of CHT. Advances to the next stop, or to the last
    /// column when there is none left -- never past the margin.
    pub(crate) fn tab_forward(&mut self, n: Count) {
        let last = self.cols.index() - 1;
        // There cannot be more stops than columns, so a larger count is the
        // same as `cols`. Without this, `CSI 65535 I` spins 65535 times once
        // the cursor is already at the margin -- bounded, but work a stream
        // should not be able to ask for.
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x >= last {
                    x = last;
                    break;
                }
                x += 1;
                if self.s.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }

    /// CBT. Backward to the previous stop, or to column 0.
    pub(crate) fn tab_back(&mut self, n: Count) {
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x == 0 {
                    break;
                }
                x -= 1;
                if self.s.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }
}
