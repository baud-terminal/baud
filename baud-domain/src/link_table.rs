//! **The `OSC 8` link table's bookkeeping**: which slot a target takes,
//! and which slots a sweep may free.
//!
//! The URIs themselves live in storage the embedder provides, `SLOTS` of
//! them at a fixed stride; this keeps each slot's key and when the next
//! sweep may run. The terminal calls it with the slices
//! (`baud-core`'s `links.rs`).

use crate::{ByteLen, Cells, LinkEpoch, LinkId, LinkKey, LinkLive, LinkSweeps, ReadLen};

/// Where a target's probe ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkProbe {
    /// A slot already holds it.
    Held(LinkId),
    /// None does, and this one is free.
    Free(LinkId),
    /// None does, and every slot it may take holds another.
    Full,
}

/// Each link slot's key, and when the table may next be swept.
pub struct LinkBook {
    /// [`LinkKey::word`] of the target each held slot was taken for.
    keys: [u32; LinkBook::SLOTS],
    /// Links that wanted a slot since the last sweep.
    since: u32,
    /// The links that must want a slot before the next sweep: `DUE`, and
    /// twice the last after each sweep that freed nothing, to `DUE_MAX`.
    wait: u32,
    /// Cells written in the reads before this one ([`LinkBook::read_ends`]).
    cells: usize,
    /// The cells written when the last sweep ran.
    swept_at: usize,
    /// Bytes read and not yet spent on a sweep ([`LinkBook::cost`]), at
    /// most `CAP`; below zero, the bytes a sweep spent before they were
    /// read, which the bytes after it pay back before the next.
    credit: isize,
    /// Bytes read and not yet spent on paying off a debt, at most
    /// `ERASE_SHARE` times `CAP`: an erase of the history or a reset pays off
    /// as much as a quarter of them, and spends four for each byte it pays
    /// ([`LinkBook::history_erased`], [`LinkBook::reset`]).
    cleared: isize,
    epoch: LinkEpoch,
    swept: LinkSweeps,
}

impl LinkBook {
    /// Slots, `NONE`'s included: every index a cell's link bits spell.
    pub const SLOTS: usize = LinkLive::SLOTS;

    /// The slots a target may take, from its home slot on: every one is
    /// read before a free one is taken, because a sweep frees slots in the
    /// middle of a run of held ones and a target can sit past a hole in its
    /// own window -- the cluster store's rule.
    ///
    /// **Sixteen, where the cluster store probes eight**, because the table
    /// is eight times smaller: with eight, `ls --hyperlink` of a 227-entry
    /// directory linked 125 targets and a hundred new ones after a sweep
    /// lost three to full windows; with sixteen, 127 -- every slot -- as
    /// with 32 and with all 127.
    pub const PROBES: usize = 16;

    /// **A sweep at most once every `SLOTS / 16` links that wanted a
    /// slot**, the cluster store's rate, so a table whose every slot is
    /// live -- where a sweep frees nothing -- does not walk the grid at
    /// every link.
    const DUE: u32 = (LinkBook::SLOTS / 16) as u32;

    /// **A sweep that freed nothing doubles the wait for the next**, to a
    /// table's worth of links, until a screen of cells is written: while
    /// every slot is named by rows still in the buffer -- a listing past
    /// 127 targets -- another sweep finds the same, and `ls` of
    /// `/usr/share/man/man1` swept every eight links, 136 times. A screen of
    /// cells, which may have carried the rows that named them out of the
    /// history, ends the wait, so a short listing after `seq 3000` sweeps
    /// at its eighth target; a sweep that frees one, and `ED 3`, bring it
    /// back to `DUE`.
    const DUE_MAX: u32 = LinkBook::SLOTS as u32;

    /// **A sweep spends a byte read for every `CELLS_PER_BYTE` cells it
    /// reads**, so a stream that keeps every slot named pays for its sweeps
    /// in bytes, whatever its bytes write. Cells alone did not bound it: a
    /// three-byte `DECALN` writes a screen, and a stream of it with eight
    /// new targets a round swept every 253 bytes. An erase of the history
    /// and a reset pay off at most one cell more a byte
    /// ([`LinkBook::ERASE_SHARE`]), so the sweeps read at most five cells a
    /// byte, beside a fixed allowance: the `CAP` the table starts with, the `CAP` an erase may
    /// hold and the one sweep a debt may finish with
    /// (`a_sweep_reads_at_most_four_cells_a_byte`,
    /// `an_erase_of_the_history_pays_off_at_most_a_cell_a_byte` and
    /// `a_reset_pays_off_at_most_a_cell_a_byte` count them). Measured, the
    /// hostile streams those tests run cost at most 7.5-8.0 ns a byte over
    /// the base, at 200x50 with a history of 10 lines.
    const CELLS_PER_BYTE: usize = 4;

    /// **An erase of the history pays off a debt from a quarter of the
    /// bytes read since the last one that paid**, at most `CAP` at once:
    /// the bytes pay for four cells of reading each, the erases for at
    /// most one more. An erase that paid off the whole debt let a stream
    /// of one scrolled linked row, `ED 3` and a new target a round sweep
    /// every round: 143-178 cells read a byte at 200x50, 150 with a
    /// history of one line.
    const ERASE_SHARE: isize = 4;

    /// **The most credit the table holds, and starts with**: 64 KiB, a
    /// quarter of a million cells' reading, so bytes read while no sweep
    /// was wanted pay for no more than that at once.
    const CAP: isize = 64 * 1024;

    pub const NEW: LinkBook = LinkBook {
        keys: [0; LinkBook::SLOTS],
        since: LinkBook::DUE,
        wait: LinkBook::DUE,
        cells: 0,
        swept_at: 0,
        credit: LinkBook::CAP,
        cleared: 0,
        epoch: LinkEpoch::ZERO,
        swept: LinkSweeps {
            sweeps: 0,
            freed: 0,
            read: 0,
        },
    };

    /// A read's cells and bytes: the cells toward the end of a wait, the
    /// bytes paid toward the next sweep.
    #[inline(always)]
    pub fn read_ends(&mut self, cells: Cells, bytes: ReadLen) {
        self.cells += cells.index();
        let b = isize::try_from(bytes.index()).unwrap_or(LinkBook::CAP);
        self.credit = self.credit.saturating_add(b).min(LinkBook::CAP);
        self.cleared = self
            .cleared
            .saturating_add(b)
            .min(LinkBook::ERASE_SHARE * LinkBook::CAP);
    }

    /// The bytes of credit a sweep that read `read` cells spends.
    fn cost(read: Cells) -> isize {
        isize::try_from(read.index() / LinkBook::CELLS_PER_BYTE).unwrap_or(isize::MAX)
    }

    /// The `n`th slot of `home`'s window, in `1..SLOTS`.
    fn slot(home: LinkId, n: usize) -> usize {
        1 + (home.index() - 1 + n) % (LinkBook::SLOTS - 1)
    }

    /// The slot `uri` under `key` takes, if any, in the URIs `links` at a
    /// stride of `stride` bytes, `lens` long; every probe is read.
    pub fn probe(
        &self,
        links: &[u8],
        lens: &[ByteLen],
        stride: usize,
        key: LinkKey,
        uri: &[u8],
    ) -> LinkProbe {
        let home = key.home(LinkBook::SLOTS);
        let mut free = None;
        for n in 0..LinkBook::PROBES {
            let i = LinkBook::slot(home, n);
            let have = lens[i].index();
            if have == 0 {
                if free.is_none() {
                    free = Some(i);
                }
            } else if self.keys[i] == key.word() && links[i * stride..][..have] == *uri {
                return LinkProbe::Held(LinkId::from_index(i));
            }
        }
        match free {
            Some(i) => LinkProbe::Free(LinkId::from_index(i)),
            None => LinkProbe::Full,
        }
    }

    /// `uri` under `key` written into the free slot `l`.
    pub fn take(
        &mut self,
        l: LinkId,
        key: LinkKey,
        uri: &[u8],
        links: &mut [u8],
        lens: &mut [ByteLen],
        stride: usize,
    ) {
        let at = l.index() * stride;
        links[at..at + uri.len()].copy_from_slice(uri);
        lens[l.index()] = ByteLen::from_index(uri.len());
        self.keys[l.index()] = key.word();
        self.wanted();
    }

    /// One more link wanted a slot.
    pub fn wanted(&mut self) {
        self.since = self.since.saturating_add(1);
    }

    /// **Whether a sweep may run** with `written` cells written so far,
    /// this read's included, on a screen of `screen`: `DUE` links have
    /// wanted a slot since the last one -- more after sweeps that freed
    /// nothing, until a screen of cells is written ([`LinkBook::DUE_MAX`])
    /// -- and the bytes read since have paid for the last
    /// ([`LinkBook::CELLS_PER_BYTE`]). What a sweep costs is counted, so a
    /// screen of cells no longer gates it: a program drawing sixty new
    /// targets a frame on the alternate screen had a few refused each
    /// frame, where its first new target came before a screen's worth.
    pub fn due(&self, written: Cells, screen: Cells) -> bool {
        let now = self.cells + written.index();
        self.since >= LinkBook::DUE
            && self.credit >= 0
            && (self.since >= self.wait || now.saturating_sub(self.swept_at) >= screen.index())
    }

    /// **Free every slot nothing in `live` names**, `live` marked from
    /// `read` cells; how many.
    ///
    /// `live` must have marked everything that can name a slot, or a cell
    /// written later is given another target.
    pub fn sweep(
        &mut self,
        lens: &mut [ByteLen],
        live: &LinkLive,
        written: Cells,
        read: Cells,
    ) -> usize {
        self.credit = self.credit.saturating_sub(LinkBook::cost(read));
        self.swept.read += read.index();
        let mut freed = 0;
        for (i, len) in lens.iter_mut().enumerate().take(LinkBook::SLOTS).skip(1) {
            if len.index() != 0 && !live.has(LinkId::from_index(i)) {
                *len = ByteLen::ZERO;
                self.keys[i] = 0;
                freed += 1;
            }
        }
        self.since = 0;
        self.swept_at = self.cells + written.index();
        self.wait = match freed {
            0 => self.wait.saturating_mul(2).min(LinkBook::DUE_MAX),
            _ => LinkBook::DUE,
        };
        self.swept.sweeps += 1;
        self.swept.freed += freed;
        if freed > 0 {
            self.epoch = self.epoch.next();
        }
        freed
    }

    /// **The history erased** (`ED 3`), rows that held links among it: the
    /// debt of the sweeps before it is paid off, from a quarter of the
    /// bytes read since the last erase that paid ([`LinkBook::ERASE_SHARE`]),
    /// and once none is left the next target with no slot sweeps at once.
    /// A listing, `clear` and a listing is how a session reaches a full
    /// table, and the rows the erase leaves are the screen's: the sweep
    /// reads what the program writes after it, where the bytes before it
    /// may have been spent on sweeps that found every slot named. An erase
    /// of a history holding none pays nothing, and one that leaves a debt
    /// leaves the wait as it was: kept as the safe direction, since a
    /// `clear` writes a screen after its erase and no test yet fails
    /// without it.
    pub fn history_erased(&mut self) {
        if self.pay_off() {
            self.wait = LinkBook::DUE;
            self.since = self.since.max(LinkBook::DUE);
        }
    }

    /// Pays the debt off from what `cleared` holds, a byte of debt for each
    /// `ERASE_SHARE` bytes read; whether none is left.
    fn pay_off(&mut self) -> bool {
        if self.credit < 0 {
            let pay = (-self.credit).min(self.cleared / LinkBook::ERASE_SHARE);
            self.credit += pay;
            self.cleared -= pay * LinkBook::ERASE_SHARE;
        }
        self.credit >= 0
    }

    /// Forget every target: every cell that could name one is being erased.
    /// The debt is paid off as an erase of the history pays it: a reset
    /// that paid off the whole debt let a stream of `RIS`, the 127 targets
    /// and a `DECALN` under a link through the history sweep the buffer
    /// every cycle, 57 cells read a byte at 200x50.
    pub fn reset(&mut self, lens: &mut [ByteLen]) {
        for len in lens.iter_mut() {
            *len = ByteLen::ZERO;
        }
        self.keys = [0; LinkBook::SLOTS];
        self.since = LinkBook::DUE;
        self.wait = LinkBook::DUE;
        self.pay_off();
        self.epoch = self.epoch.next();
    }

    /// See [`LinkEpoch`].
    pub fn epoch(&self) -> LinkEpoch {
        self.epoch
    }

    /// See [`LinkSweeps`].
    pub fn sweeps(&self) -> LinkSweeps {
        self.swept
    }
}
