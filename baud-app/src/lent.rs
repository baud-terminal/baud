//! **What a window lends as `'static`, and gives back when it closes.**
//!
//! A window's terminal borrows its storage and the kitty files reader for
//! as long as it lives, and its renderer borrows its faces and the font
//! files under them; the window's driver lives behind its view as a
//! `'static` responder, so those loans are `'static` too. Each is made
//! here together with what borrows it, in one type whose drop ends the
//! borrower first and then gives the loan back:
//!
//! * [`Screen`]: the terminal, its storage, the kitty files reader it is
//!   lent, and whatever else its surfaces live in -- on the cell path the
//!   two GPU buffers the core's grids are lent from.
//! * [`Drawing`]: the renderer and the fonts it draws from: the parsed
//!   faces freed and the font files given back (`pty::Held::release`),
//!   each file unmapped once no other window's fonts count it.
//!
//! So a closed window hands back everything it was opened with, and the
//! order the window's own fields are declared in decides nothing.
//!
//! `unsafe` is allowed here for the loans: a box taken apart into the
//! reference lent and given back once its borrower has dropped, and the
//! fonts' release ([`Drawing`]'s drop). The references lent go only into
//! the borrower each type holds -- [`Screen::new`]'s `build` makes the
//! terminal of them and hands it back -- and nothing here hands out the
//! borrower by value. The types do not stop a `&mut` to the terminal or
//! the renderer from swapping it for another window's, which would free
//! what the swapped one still borrows; the app never does
//! (`the_app_never_swaps_a_windows_terminal_or_renderer`).
#![allow(unsafe_code)]

use core::mem::ManuallyDrop;
use core::ptr::NonNull;

use baud_core::{Owned, Term};

/// **A window's terminal with everything it borrows**: dropped, the
/// terminal goes first, then its kitty files reader and its storage, then
/// `K`, what its surfaces live in when the storage does not hold them.
pub struct Screen<K> {
    term: ManuallyDrop<Term<'static>>,
    owned: NonNull<Owned>,
    files: NonNull<crate::kitty_files::LocalFiles>,
    keep: ManuallyDrop<K>,
}

impl<K> Screen<K> {
    /// The terminal `build` makes of `owned`, lent `files` as its kitty
    /// files reader, with `keep` held until both are given back. `build`
    /// lends the storage to the terminal it returns and to nothing else.
    pub fn new(
        owned: Owned,
        files: crate::kitty_files::LocalFiles,
        keep: K,
        build: impl FnOnce(&'static mut Owned) -> Term<'static>,
    ) -> Screen<K> {
        let owned = NonNull::from(Box::leak(Box::new(owned)));
        let files = NonNull::from(Box::leak(Box::new(files)));
        // SAFETY: a live allocation this owns, lent once, to the terminal
        // `build` makes, which this holds and drops before the box is
        // given back.
        let mut term = build(unsafe { &mut *owned.as_ptr() });
        // SAFETY: as above, lent once, to the terminal.
        term.set_kitty_files(unsafe { &mut *files.as_ptr() });
        Screen {
            term: ManuallyDrop::new(term),
            owned,
            files,
            keep: ManuallyDrop::new(keep),
        }
    }
}

impl<K> core::ops::Deref for Screen<K> {
    type Target = Term<'static>;
    fn deref(&self) -> &Term<'static> {
        &self.term
    }
}

impl<K> core::ops::DerefMut for Screen<K> {
    fn deref_mut(&mut self) -> &mut Term<'static> {
        &mut self.term
    }
}

impl<K> Drop for Screen<K> {
    fn drop(&mut self) {
        // SAFETY: dropped once, here, and never read after.
        unsafe { ManuallyDrop::drop(&mut self.term) };
        // SAFETY: the boxes `new` took apart, each given back once, now
        // that the terminal they were lent to has dropped.
        drop(unsafe { Box::from_raw(self.files.as_ptr()) });
        // SAFETY: as above.
        drop(unsafe { Box::from_raw(self.owned.as_ptr()) });
        // SAFETY: dropped once, here, after the terminal that wrote into
        // it.
        unsafe { ManuallyDrop::drop(&mut self.keep) };
    }
}

/// **A window's renderer with the fonts it draws from**: dropped, the
/// renderer goes first, then the fonts, whose faces are freed and whose
/// files are given back.
pub struct Drawing {
    renderer: ManuallyDrop<baud_render::Renderer<'static, 'static>>,
    fonts: ManuallyDrop<crate::fonts::Fonts>,
}

impl Drawing {
    /// `renderer`, which draws from `fonts`' faces or from faces compiled
    /// in, and nothing else that a release frees.
    pub fn new(
        renderer: baud_render::Renderer<'static, 'static>,
        fonts: crate::fonts::Fonts,
    ) -> Drawing {
        Drawing {
            renderer: ManuallyDrop::new(renderer),
            fonts: ManuallyDrop::new(fonts),
        }
    }

    pub fn fonts(&self) -> &crate::fonts::Fonts {
        &self.fonts
    }

    pub fn fonts_mut(&mut self) -> &mut crate::fonts::Fonts {
        &mut self.fonts
    }

    /// The renderer and the fonts together, for a reload that restacks one
    /// on the other (`host::refont`, `host::follow_gaps`).
    pub fn both(
        &mut self,
    ) -> (
        &mut baud_render::Renderer<'static, 'static>,
        &mut crate::fonts::Fonts,
    ) {
        (&mut self.renderer, &mut self.fonts)
    }
}

impl core::ops::Deref for Drawing {
    type Target = baud_render::Renderer<'static, 'static>;
    fn deref(&self) -> &baud_render::Renderer<'static, 'static> {
        &self.renderer
    }
}

impl core::ops::DerefMut for Drawing {
    fn deref_mut(&mut self) -> &mut baud_render::Renderer<'static, 'static> {
        &mut self.renderer
    }
}

impl Drop for Drawing {
    fn drop(&mut self) {
        let held = self.fonts.take_held();
        // SAFETY: each dropped once, here, and never read after: the
        // renderer first, whose stack is its one borrow of the faces, then
        // the fonts, whose stacks and lookup are the rest.
        unsafe {
            ManuallyDrop::drop(&mut self.renderer);
            ManuallyDrop::drop(&mut self.fonts);
        }
        // SAFETY: nothing borrows these faces or bytes now: the renderer
        // and the fonts made of them have dropped above, and a GPU frame
        // reads the atlas the renderer owned, never the faces.
        let _ = unsafe { held.release() };
    }
}

#[cfg(test)]
mod tests {
    use super::{Drawing, Screen};
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::heap::live;

    const WS: WinSize = WinSize {
        ws_col: 80,
        ws_row: 24,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// Counts its drops into a shared cell, standing for the GPU buffers a
    /// `Screen` holds.
    struct Dropped(Rc<Cell<usize>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// A window's storage as the app provisions it, at `ws`.
    fn storage(ws: &WinSize) -> Owned {
        let cfg = crate::config::Config::defaults();
        Owned::new(ws)
            .with_images(cfg.images)
            .with_clipboard(cfg.clipboard.budget)
            .with_printer(crate::print::budget(cfg.printing))
            .with_notices(crate::notify::BUDGET)
    }

    fn screen(dropped: &Rc<Cell<usize>>) -> Screen<Dropped> {
        let mut s = Screen::new(
            storage(&WS),
            crate::kitty_files::LocalFiles::new(),
            Dropped(Rc::clone(dropped)),
            |owned| Term::new(owned.storage(), &WS),
        );
        feed(&mut s, "a window's text\r\n".repeat(200).as_bytes());
        s
    }

    /// **A closed window's terminal gives back every byte it was opened
    /// with**: its storage, image store and kitty files reader, and what
    /// its surfaces live in, dropped once and after the terminal. The
    /// control: a `Screen` forgotten, as the leak was, keeps at least its
    /// storage's bytes.
    #[test]
    fn a_closed_windows_screen_gives_back_every_byte_it_was_opened_with() {
        let dropped = Rc::new(Cell::new(0));
        // A first window, so what the process makes once is made.
        drop(screen(&dropped));
        let before = live();
        let s = screen(&dropped);
        let opened = live() - before;
        assert!(
            opened > 1 << 20,
            "the control: a window's storage is megabytes, {opened}"
        );
        drop(s);
        assert_eq!(live(), before, "every byte given back");
        assert_eq!(dropped.get(), 2, "each window's buffers dropped once");
        let kept = screen(&dropped);
        std::mem::forget(kept);
        assert!(
            live() - before >= opened,
            "the control: forgotten, as leaked, it keeps them"
        );
    }

    /// The config of a stack of `path`'s face, resolved.
    fn config(path: &std::path::Path) -> crate::config::Config {
        let mut cfg = crate::config::Config::defaults();
        assert!(
            cfg.apply(&format!("fonts = [{:?}]\n", path.display().to_string()))
                .is_empty()
        );
        crate::host::resolved(&mut cfg);
        cfg
    }

    /// A drawing of `cfg`'s faces, at 8x2 cells, and where its first
    /// face's file is mapped.
    fn drawing(cfg: &crate::config::Config) -> (Drawing, *const u8) {
        let (data, mut held) = crate::host::map_faces(cfg);
        let at = data[0].as_ptr();
        let faces = crate::host::parse_faces(cfg, &data, &mut held);
        let fonts = crate::fonts::Fonts::new(cfg, data, faces, held);
        let renderer = baud_render::Renderer::new(
            crate::host::stack(&fonts),
            baud_render::PxSize::of(26.0).expect("a size"),
            baud_render::Theme::dark(),
            baud_render::Blend::Native,
            Cols::of_index(8),
            Rows::of_index(2),
        )
        .expect("metrics");
        (Drawing::new(renderer, fonts), at)
    }

    /// One frame of `text`, drawn.
    fn draw(d: &mut Drawing, text: &[u8]) -> Vec<u32> {
        let ws = WinSize {
            ws_row: 2,
            ws_col: 8,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, text);
        let (w, h) = d.whole_size();
        let area = baud_font::Extent::new(w, h).cells().expect("an area");
        let mut frame = vec![0u32; area];
        let mut target = baud_render::Target::new(&mut frame, w, w, h).expect("a target");
        let _ = d.render(&mut t, &mut target, baud_render::Damage::Full);
        frame
    }

    /// **A closed window's renderer gives back its faces and its font
    /// file**: two windows over one file share its mapping, the first's
    /// close leaves the second drawing from it, the second's unmaps it,
    /// and both closes give back every byte their faces and renderers
    /// took. The control: while both are open the file has two holders.
    #[test]
    fn a_closed_windows_drawing_gives_back_its_faces_and_font_file() {
        let dir = std::env::temp_dir().join(format!("baud-lent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Face.ttf");
        std::fs::write(
            &path,
            std::fs::read(format!(
                "{}/../baud-font/tests/fonts/FiraCode-Regular.ttf",
                env!("CARGO_MANIFEST_DIR")
            ))
            .expect("a test font"),
        )
        .unwrap();
        let cfg = config(&path);
        // A first window, so what the process makes once is made.
        let (mut first, _) = drawing(&cfg);
        let _ = draw(&mut first, b"Ag");
        drop(first);
        let before = live();
        let (mut a, at) = drawing(&cfg);
        let (mut b, bt) = drawing(&cfg);
        assert!(core::ptr::eq(at, bt), "one mapping of the file");
        let one = crate::pty::Holders::ONE;
        assert_eq!(
            crate::pty::holders(at),
            Some(one.more()),
            "the control: both windows count it"
        );
        let drawn = draw(&mut a, b"Ag");
        drop(a);
        assert_eq!(
            crate::pty::holders(at),
            Some(one),
            "the second still counts it"
        );
        assert_eq!(draw(&mut b, b"Ag"), drawn, "and draws from it");
        drop(drawn);
        drop(b);
        assert_eq!(crate::pty::holders(at), None, "the last close unmaps it");
        assert_eq!(live(), before, "every byte given back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The app never swaps a window's terminal or renderer out of its
    /// `Screen` or `Drawing`**, which would leave it borrowing what the
    /// other's drop frees: no `mem::swap`, `mem::replace` or `mem::take`
    /// in the app's driver takes either.
    #[test]
    fn the_app_never_swaps_a_windows_terminal_or_renderer() {
        const SRC: &str = include_str!("app.rs");
        for verb in ["mem::swap(", "mem::replace(", "mem::take("] {
            for (at, _) in SRC.match_indices(verb) {
                let arg: String = SRC[at + verb.len()..].chars().take(40).collect();
                let first = arg.split(',').next().unwrap_or("");
                for field in [".term", ".renderer"] {
                    assert!(
                        !first.trim_end().ends_with(field),
                        "{verb}{arg}: a window's {field} swapped"
                    );
                }
            }
        }
    }

    /// This process's resident size and footprint, in bytes.
    fn memory() -> (u64, u64) {
        use crate::sys::{TASK_VM_INFO, TaskVmInfo, mach_task_self_, task_info};
        let mut info = TaskVmInfo::default();
        let words = core::mem::size_of::<TaskVmInfo>() / core::mem::size_of::<i32>();
        let mut count = u32::try_from(words).unwrap();
        // SAFETY: `info` is the flavor's struct through `phys_footprint`,
        // `count` its length in `natural_t`s, as the probe holds them.
        let kr = unsafe {
            task_info(
                mach_task_self_,
                TASK_VM_INFO,
                (&raw mut info).cast(),
                &raw mut count,
            )
        };
        assert_eq!(kr, 0, "task_info");
        (info.resident_size.0, info.phys_footprint.0)
    }

    /// **Twenty windows' state opened and closed, and what the process
    /// holds after each**, for the record rather than a gate: each window
    /// is the storage the app provisions at its largest grid, written a
    /// screen and its history deep, and the default config's fonts and a
    /// renderer that has drawn a frame. `BAUD_LEAK=1` forgets each window
    /// instead, as the app did before a close gave anything back. Run with
    /// `--run-ignored only --no-capture`.
    #[test]
    #[ignore = "a measurement, printed"]
    fn twenty_windows_opened_and_closed() {
        let leak = std::env::var_os("BAUD_LEAK").is_some();
        let mut cfg = crate::config::Config::defaults();
        crate::host::resolved(&mut cfg);
        let geom = crate::host::host_geometry();
        let max = crate::host::MAX_GRID;
        let line = "0123456789abcdefghijklmnopqrstuvwxyz".repeat(12);
        let text = format!("{}\r\n", &line[..usize::from(max.ws_col)]).repeat(2400);
        let mb = |b: u64| b as f64 / (1u64 << 20) as f64;
        let (r0, f0) = memory();
        eprintln!(
            "before: resident {:.1} MB, footprint {:.1} MB",
            mb(r0),
            mb(f0)
        );
        for n in 1..=20 {
            let mut screen = Screen::new(
                Owned::with_geometry(&max, &geom)
                    .with_images(cfg.images)
                    .with_clipboard(cfg.clipboard.budget)
                    .with_printer(crate::print::budget(cfg.printing))
                    .with_notices(crate::notify::BUDGET),
                crate::kitty_files::LocalFiles::new(),
                (),
                |owned| Term::new(owned.storage(), &max),
            );
            feed(&mut screen, text.as_bytes());
            let (mut d, _) = drawing(&cfg);
            let frame = draw(&mut d, line.as_bytes());
            drop(frame);
            if leak {
                std::mem::forget(screen);
                std::mem::forget(d);
            } else {
                drop(screen);
                drop(d);
            }
            let (r, f) = memory();
            eprintln!(
                "window {n:2} closed: resident {:.1} MB, footprint {:.1} MB, held {} B",
                mb(r),
                mb(f),
                live()
            );
        }
    }
}
