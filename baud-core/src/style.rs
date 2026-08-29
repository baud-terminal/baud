use crate::*;

impl<'a> Term<'a> {
    #[inline(always)]
    /// The rendition a cell's [`StyleId`] stands for.
    ///
    /// A renderer needs this: the cell carries an index into a 64-entry
    /// direct-mapped table, and the colours, attributes and font live in the
    /// table rather than in the cell. Reads the slot back rather than trusting
    /// the id, because the table evicts on collision -- the same check DECSCA
    /// protection needed and did not have.
    pub fn style_of(&self, cell: PackedCell) -> StyleWord {
        let i = cell.style.index();
        if self.s.tmpl_valid[i] {
            self.s.tmpl_key[i]
        } else {
            StyleWord::DEFAULT
        }
    }

    pub(crate) fn intern_style(&mut self, raw: StyleWord) -> StyleId {
        let k = StyleId::bucket(raw, NSTYLE);
        if self.s.tmpl_valid[k] && self.s.tmpl_key[k] == raw {
            self.tmpl_hits.inc();
        } else {
            self.s.tmpl_valid[k] = true;
            self.s.tmpl_key[k] = raw;
            self.tmpl_builds.inc();
        }
        StyleId::from_index(k)
    }

    pub(crate) fn apply_sgr(&mut self) {
        let n = self.nparams();
        // A `ParamIdx`, not a `usize` counter: it indexes the parameter array
        // *and* realigns the sub-parameter mask, and those two must step
        // together. Two bare integers here is how the colon form would come to
        // be read correctly for the first attribute and wrongly for the rest.
        let mut i = ParamIdx::FIRST;
        while i.raw_index() < n {
            match baud_domain::SgrAttr::parse(Params::with_subs(
                &self.params[i.raw_index()..n],
                self.psub.drop_first(i),
            )) {
                Some((attr, used)) => {
                    attr.fold_into(&mut self.style_raw);
                    i = i.advance(used.at_least_one_count());
                }
                None => break,
            }
        }
        self.style = self.intern_style(self.style_raw);
    }

    /// Recombine DECSCA protection with the active character set.
    ///
    /// Called whenever either input changes, so the write path can read
    /// `self.flags` as one value. Doing this per cell instead would put a
    /// second load and an OR inside the loop that `put_ascii_run` exists to
    /// keep tight.
    pub(crate) fn refresh_flags(&mut self) {
        let base = match self.g[self.gl.index()] {
            Charset::DecGraphics => self.protect.with(CellFlags::GRAPHICS),
            _ => self.protect,
        };
        self.flags = base.with_link(self.link);
    }
}
