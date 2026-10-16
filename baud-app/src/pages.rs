//! **Freed image pages back to the operating system**: the
//! [`PageRelease`] both drivers lend the core, so a picture shown and
//! deleted leaves the terminal at its idle size rather than at its high
//! water. The image arena and the kitty PNG scratch come from zeroed
//! memory and are committed only where an image writes
//! (`Owned::with_images`); the core hands this the whole pages a freed
//! image or a Sixel's claim leaves wholly free and those of the scratch a
//! PNG decode is done with, page-aligned and held by nothing live, and
//! this gives them back.
//!
//! Linux: `madvise(MADV_DONTNEED)`, after which a private anonymous page
//! reads zero and stops counting as resident. Darwin: a fresh anonymous
//! mapping over the pages (`MAP_FIXED`), because no `madvise` advice
//! lowered the resident size there (`sys.rs`); the new pages read zero,
//! as Linux's do. Each reports whether the call succeeded
//! ([`PagesBack`]), and only taken pages count. What a failed call
//! leaves neither manual page says (`[[source: linux-madvise-2]]`,
//! `[[source: darwin-mmap-2]]`): old bytes the core does not mind, since
//! what a released page holds is unspecified ([`PageRelease`]) and every
//! image writes its span before it is shown; an unmapped page, which
//! POSIX allows of a failed `MAP_FIXED`, it would, and no failure was
//! seen here.
//!
//! This release's `unsafe` is here and in `sys.rs`, and each call says why.

#![allow(unsafe_code)]

use baud_domain::{PageRelease, PagesBack};

/// The release both drivers lend ([`baud_core::Term::set_page_release`]):
/// the system's page size and [`give_back`]. `None` if the system answers
/// a page size that is not a power of two, and then pages stay committed.
pub fn release() -> Option<PageRelease> {
    // SAFETY: `getpagesize` takes nothing and only answers.
    let page = usize::try_from(unsafe { crate::sys::getpagesize() }).ok()?;
    PageRelease::new(page, give_back)
}

/// Give `pages` back, and say whether the system took them. The core
/// hands only whole pages no live image holds; a span that is not
/// page-aligned is refused here too, because Linux's `madvise` rounds a
/// length up and would zero the next page.
fn give_back(pages: &mut [u8]) -> PagesBack {
    let Some(size) = release().map(PageRelease::page) else {
        return PagesBack::Kept;
    };
    if pages.is_empty()
        || pages.as_ptr().align_offset(size) != 0
        || !pages.len().is_multiple_of(size)
    {
        return PagesBack::Kept;
    }
    let at = pages.as_mut_ptr().cast::<core::ffi::c_void>();
    #[cfg(target_os = "macos")]
    {
        use crate::sys::{MAP_ANON, MAP_FIXED, MAP_PRIVATE, PROT_READ, PROT_WRITE};
        // SAFETY: `pages` is an exclusive borrow of whole, page-aligned
        // pages of a live allocation; the mapping replaces them in place
        // with zero-filled pages of the same protection, which is what
        // writing zeros through the borrow would leave, and `Pixel` and
        // `u8` take any bits. `mmap.2` says what a successful `MAP_FIXED`
        // replaces and nothing of what a failed one leaves
        // (`[[source: darwin-mmap-2]]`); a failure was never seen here.
        let mapped = unsafe {
            crate::sys::mmap(
                at,
                pages.len(),
                PROT_READ | PROT_WRITE,
                MAP_FIXED | MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        // Success is the address asked for; a failure is `MAP_FAILED`.
        match mapped == at {
            true => PagesBack::Released,
            false => PagesBack::Kept,
        }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: as above; a private anonymous page reads zero "after a
        // successful" call, and the page says nothing of a failed one
        // (`[[source: linux-madvise-2]]`).
        let done = unsafe {
            crate::sys::madvise(
                at,
                crate::sys::MapLen(pages.len()),
                crate::sys::MADV_DONTNEED,
            )
        };
        match done {
            0 => PagesBack::Released,
            _ => PagesBack::Kept,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};
    use baud_domain::{Chan, DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId};

    /// A read, as the drivers' are at most.
    const READ: usize = 4096;

    /// Feed `bytes` in reads of [`READ`], dropping the replies, then
    /// finish any deferred decode.
    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        for chunk in bytes.chunks(READ) {
            t.write_at(BytePos::ZERO, chunk);
            t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            t.clear_reply();
        }
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        t.clear_reply();
    }

    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// RFC 4648 base64, padded.
    fn b64(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() / 3 * 4 + 4);
        for q in data.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                let six = usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0);
                out.push(if k <= q.len() { B64[six] } else { b'=' });
            }
        }
        out
    }

    /// One graphics command.
    fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = b"\x1b_G".to_vec();
        v.extend_from_slice(ctrl.as_bytes());
        if !payload.is_empty() {
            v.push(b';');
            v.extend(b64(payload));
        }
        v.extend_from_slice(b"\x1b\\");
        v
    }

    /// RGBA bytes of a `w` x `h` image, every pixel opaque and its colour
    /// a function of `seed` and where it is, so two images differ in every
    /// pixel and none is zero.
    fn rgba(w: ImagePx, h: ImagePx, seed: Chan) -> Vec<u8> {
        let seed = seed.get();
        let mut v = Vec::new();
        for y in 0..h.raw() {
            for x in 0..w.raw() {
                v.extend_from_slice(&[
                    seed ^ x.to_le_bytes()[0],
                    seed.wrapping_add(y.to_le_bytes()[0]),
                    seed | 1,
                    0xff,
                ]);
            }
        }
        v
    }

    /// The pixel words of `rgba`, as the store keeps them.
    fn words(rgba: &[u8]) -> Vec<u32> {
        rgba.chunks(4)
            .map(|p| 0xff00_0000 | u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
            .collect()
    }

    /// The pixels the store holds for kitty image `id`.
    fn shown(t: &Term<'_>, id: KittyId) -> Vec<u32> {
        let image = t.images().kitty_image(id).expect("stored");
        let v = t.images().image(image).expect("shown");
        v.pixels.iter().map(|p| p.bits()).collect()
    }

    /// Where kitty image `id`'s pixels start and end in memory.
    fn bounds(t: &Term<'_>, id: KittyId) -> core::ops::Range<*const baud_domain::Pixel> {
        let image = t.images().kitty_image(id).expect("stored");
        t.images()
            .image(image)
            .expect("shown")
            .pixels
            .as_ptr_range()
    }

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    /// **A neighbour on a shared page is untouched, and a picture stored
    /// where pages went back reads back exactly**, through the real
    /// release: on this Mac a released page reads zero, so a page given
    /// back that a live image still used, or a re-stored image that left
    /// a pixel unwritten, shows as a zero where a colour was.
    #[test]
    fn a_neighbour_keeps_its_page_and_a_restored_picture_reads_back_exactly() {
        let page = release().expect("a release").page();
        let px = page / 4;
        // A: three and a half pages of pixels, so whole pages lie inside
        // it whatever the arena's alignment and its last page is shared;
        // B right after it, starting on that page.
        let (aw, ah) = (u16::try_from(px / 2).unwrap(), 7u16);
        let (bw, bh) = (64u16, 16u16);
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(px * 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        t.set_page_release(release().expect("a release"));
        let (im, ch) = (ImagePx::of, Chan::from_u8);
        let (a, b, c) = (
            rgba(im(aw), im(ah), ch(0x31)),
            rgba(im(bw), im(bh), ch(0x5c)),
            rgba(im(aw), im(ah), ch(0xa7)),
        );
        feed(&mut t, &apc(&format!("a=t,f=32,s={aw},v={ah},i=1"), &a));
        feed(&mut t, &apc(&format!("a=t,f=32,s={bw},v={bh},i=2"), &b));
        let (ra, rb) = (bounds(&t, KittyId::of(1)), bounds(&t, KittyId::of(2)));
        assert_eq!(ra.end, rb.start, "B is stored right after A");
        // **The control**: A's end falls inside a page, so B shares it.
        assert_ne!(
            ra.end.cast::<u8>().align_offset(page),
            0,
            "the boundary is mid-page"
        );
        feed(&mut t, &apc("a=d,d=I,i=1", &[]));
        assert_eq!(shown(&t, KittyId::of(2)), words(&b), "B is pixel-exact");
        assert!(
            t.images().bytes_released >= 2 * page,
            "A's whole pages went back: {}",
            t.images().bytes_released
        );
        feed(&mut t, &apc(&format!("a=t,f=32,s={aw},v={ah},i=3"), &c));
        assert_eq!(
            bounds(&t, KittyId::of(3)).start,
            ra.start,
            "C took A's span"
        );
        assert_eq!(shown(&t, KittyId::of(3)), words(&c), "C reads back exactly");
        assert_eq!(
            shown(&t, KittyId::of(2)),
            words(&b),
            "B is still pixel-exact"
        );
    }

    /// **An image whose placement goes while another stays keeps its
    /// pages**: only a freed span goes back. Its pixels are read after the
    /// delete, so a page given back under a live image reads zero.
    #[test]
    fn an_image_that_keeps_a_placement_keeps_its_pages() {
        let page = release().expect("a release").page();
        let px = page / 4;
        let (w, h) = (u16::try_from(px / 2).unwrap(), 9u16);
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(px * 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        t.set_page_release(release().expect("a release"));
        let a = rgba(ImagePx::of(w), ImagePx::of(h), Chan::from_u8(0x66));
        feed(&mut t, &apc(&format!("a=T,f=32,s={w},v={h},i=4,p=1"), &a));
        feed(&mut t, &apc("a=p,i=4,p=2", &[]));
        // Uppercase: the image goes too once no placement is left, and one
        // is.
        feed(&mut t, &apc("a=d,d=I,i=4,p=1", &[]));
        assert_eq!(t.images().placements().count(), 1, "one placement left");
        assert_eq!(shown(&t, KittyId::of(4)), words(&a), "its pixels are whole");
        assert_eq!(t.images().bytes_released, 0, "nothing was freed");
    }

    /// A stored-deflate PNG of 8-bit RGB `rgb`, `w` x `h`, every row filter
    /// 0: a valid file the core decodes, written here because the picture
    /// is not the thing under test.
    fn png(w: ImagePx, h: ImagePx, rgb: &[u8]) -> Vec<u8> {
        let crc = |bytes: &[u8]| {
            let mut c = !0u32;
            for &b in bytes {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xedb8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        };
        let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            let at = out.len();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let c = crc(&out[at..]);
            out.extend_from_slice(&c.to_be_bytes());
        };
        let row = w.index() * 3;
        let mut raw = Vec::new();
        for r in rgb.chunks(row) {
            raw.push(0);
            raw.extend_from_slice(r);
        }
        let mut z = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
        for (k, b) in blocks.iter().enumerate() {
            z.push(u8::from(k + 1 == blocks.len()));
            let n = u16::try_from(b.len()).unwrap();
            z.extend_from_slice(&n.to_le_bytes());
            z.extend_from_slice(&(!n).to_le_bytes());
            z.extend_from_slice(b);
        }
        let (mut s1, mut s2) = (1u32, 0u32);
        for &b in &raw {
            s1 = (s1 + u32::from(b)) % 65521;
            s2 = (s2 + s1) % 65521;
        }
        z.extend_from_slice(&(s2 << 16 | s1).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&u32::from(w.raw()).to_be_bytes());
        ihdr.extend_from_slice(&u32::from(h.raw()).to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    /// A resident size, in bytes.
    #[derive(Clone, Copy)]
    struct Resident(u64);

    /// This process's resident size.
    #[cfg(target_os = "macos")]
    fn resident() -> Resident {
        use crate::sys::{MACH_TASK_BASIC_INFO, MachTaskBasicInfo, mach_task_self_, task_info};
        let mut info = MachTaskBasicInfo::default();
        let words = core::mem::size_of::<MachTaskBasicInfo>() / core::mem::size_of::<i32>();
        let mut count = u32::try_from(words).unwrap();
        // SAFETY: `info` is the flavor's struct, `count` its length in
        // `natural_t`s, as the probe holds them to the headers.
        let kr = unsafe {
            task_info(
                mach_task_self_,
                MACH_TASK_BASIC_INFO,
                (&raw mut info).cast(),
                &raw mut count,
            )
        };
        assert_eq!(kr, 0, "task_info");
        Resident(info.resident_size.0)
    }

    /// This process's resident size: `statm`'s second field, in pages.
    #[cfg(target_os = "linux")]
    fn resident() -> Resident {
        let statm = std::fs::read_to_string("/proc/self/statm").expect("statm");
        let pages: u64 = statm
            .split(' ')
            .nth(1)
            .and_then(|n| n.parse().ok())
            .expect("rss");
        let page = release().expect("a release").page();
        Resident(pages * u64::try_from(page).unwrap())
    }

    /// The child's report: resident bytes idle, with the picture shown,
    /// and after it was deleted, for each of three cycles.
    const CHILD: &str = "BAUD_PAGES_CHILD";

    /// **The measured half, in a process of its own**: the harness runs
    /// tests as threads of one process, and another test's pages would be
    /// in the number. The parent runs this test binary again on
    /// [`rss_child`] and reads what it prints.
    fn measure(kind: &str) -> Vec<(u64, u64, u64)> {
        let out = std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args([
                "--exact",
                "pages::tests::rss_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, kind)
            .output()
            .expect("run the child");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "the child failed:\n{text}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let rows: Vec<(u64, u64, u64)> = text
            .lines()
            .filter_map(|l| l.find("rss ").map(|i| &l[i + 4..]))
            .map(|l| {
                let n: Vec<u64> = l.split(' ').map(|x| x.parse().expect("a number")).collect();
                (n[0], n[1], n[2])
            })
            .collect();
        // **The control**: a child that printed nothing passes nothing.
        assert_eq!(rows.len(), 3, "three cycles reported:\n{text}");
        rows
    }

    /// Two MB, the margin over idle a deleted picture may leave.
    const MARGIN: u64 = 2_000_000;

    fn assert_released(kind: &str) {
        let rows = measure(kind);
        // Idle is the first cycle's, so a page kept by one cycle shows in
        // every cycle after it.
        let idle = rows[0].0;
        for (_, shown, after) in rows {
            eprintln!("{kind}: idle {idle} shown {shown} after {after}");
            // **The control**: the picture committed its pages, or the
            // fall proves nothing.
            assert!(
                shown > idle + 8_000_000,
                "{kind}: the picture was committed: {idle} -> {shown}"
            );
            assert!(
                after <= idle + MARGIN,
                "{kind}: after the delete {after} bytes resident, idle was {idle}"
            );
        }
    }

    /// **A 1920x1080 PNG shown and deleted leaves the terminal within 2 MB
    /// of idle**: its span in the arena and the scratch its decode wrote
    /// both go back. Measured resident, not peak: `/usr/bin/time -l` and
    /// `getrusage` report the high water, which no release lowers.
    #[test]
    fn a_deleted_png_gives_its_pages_back() {
        assert_released("png");
    }

    /// The same for a 1920x1080 picture sent as `f=32`, which writes the
    /// arena and no scratch.
    #[test]
    fn a_deleted_rgba_picture_gives_its_pages_back() {
        assert_released("rgba");
    }

    /// One cycle's resident bytes: idle, shown, after.
    type Cycle = (u64, u64, u64);

    /// Each cycle's `(idle, shown, after)` for `kind` with the release
    /// lent, and the same without it: the control that the scenario
    /// commits what the release then gives back.
    fn lent_and_not(kind: &str) -> (Vec<Cycle>, Vec<Cycle>) {
        let lent = measure(kind);
        let not = measure(&format!("{kind},nolend"));
        for (l, n) in lent.iter().zip(&not) {
            eprintln!("{kind}: lent {l:?} not lent {n:?}");
        }
        (lent, not)
    }

    /// **Pictures that share every page with a neighbour give them all
    /// back once all are gone**: 32 pictures of 4095 pixels, one byte
    /// short of a 16 KiB page each, side by side, so no picture holds a
    /// whole page and every page is shared, then `RIS`. Without the
    /// release they stay; with it they go as the last of each page's
    /// pictures does. Judged from the second cycle: the first also
    /// commits memory the terminal keeps on first use, with or without a
    /// release (0.23 MB here).
    #[test]
    fn pictures_that_share_every_page_give_them_back() {
        let page = u64::try_from(release().expect("a release").page()).unwrap();
        let (lent, not) = lent_and_not("tiles");
        // **The control**: without the release the pages stay.
        let (n_idle, _, n_after) = not[0];
        assert!(
            n_after > n_idle + 24 * page,
            "kept without a release: {n_idle} -> {n_after}"
        );
        for &(idle, shown, after) in &lent[1..] {
            assert!(
                shown > idle + 24 * page,
                "committed again: {idle} -> {shown}"
            );
            assert!(
                after <= idle + 4 * page,
                "after the reset {after} bytes resident, idle was {idle}"
            );
        }
    }

    /// **A refused Sixel's rows go back**: every image slot is held, so a
    /// 1920x1080 Sixel decodes into the arena and is refused at its
    /// terminator; what it wrote goes back then, not at a reset.
    #[test]
    fn a_refused_sixel_gives_its_rows_back() {
        let (lent, not) = lent_and_not("sixel-refused");
        // **The control**: without the release the rows stay.
        let (n_idle, n_shown, _) = not[0];
        assert!(
            n_shown > n_idle + 8_000_000,
            "kept without a release: {n_idle} -> {n_shown}"
        );
        let idle = lent[0].0;
        for (_, shown, after) in lent {
            assert!(
                shown.max(after) <= idle + MARGIN,
                "after the Sixel {shown} and the reset {after}, idle was {idle}"
            );
        }
    }

    /// **A moved Sixel's rows go back**: a 1920x1080 picture is deleted
    /// between two 1x1s, and a 1920x1080 Sixel decodes past them and moves
    /// into the hole at its terminator; what it left goes back at once,
    /// so only the Sixel is resident, and after `RIS` nothing is.
    #[test]
    fn a_moved_sixel_gives_back_the_rows_it_left() {
        let (lent, not) = lent_and_not("sixel-moved");
        let image = 1920 * 1080 * 4;
        // **The control**: without the release the Sixel and the rows it
        // left are both resident, and stay.
        let (n_idle, n_shown, n_after) = not[0];
        assert!(
            n_shown > n_idle + 2 * image - MARGIN && n_after > n_idle + 2 * image - MARGIN,
            "kept without a release: {n_idle} -> {n_shown} -> {n_after}"
        );
        let idle = lent[0].0;
        for (_, shown, after) in lent {
            assert!(
                shown <= idle + image + MARGIN && after <= idle + MARGIN,
                "the Sixel shown {shown} and after the reset {after}, idle was {idle}"
            );
        }
    }

    /// A green Sixel `w` x `h` with its raster given.
    fn green_sixel(w: ImagePx, h: ImagePx) -> Vec<u8> {
        let (w, h) = (w.raw(), h.raw());
        let mut v = format!("\x1bP9;1;0q\"1;1;{w};{h}#1;2;0;100;0#1").into_bytes();
        for band in 0..h.div_ceil(6) {
            if band > 0 {
                v.push(b'-');
            }
            v.extend(format!("!{w}~").into_bytes());
        }
        v.extend_from_slice(b"\x1b\\");
        v
    }

    /// The child [`measure`] runs: the terminal the kitty-follow
    /// measurement built -- 400x150, the app's image budget -- with the
    /// release lent, one 1920x1080 picture shown and deleted three times.
    /// Ignored, so only the parent runs it.
    #[test]
    #[ignore = "run by measure() in a process of its own"]
    fn rss_child() {
        let Ok(kind) = std::env::var(CHILD) else {
            return;
        };
        let ws = WinSize {
            ws_row: 150,
            ws_col: 400,
        };
        if kind.starts_with("tiles") || kind.starts_with("sixel") {
            return rss_child_reset(&kind, ws);
        }
        let (w, h) = (ImagePx::of(1920), ImagePx::of(1080));
        let seed = Chan::from_u8(0x42);
        let show = match kind.as_str() {
            "png" => {
                let rgb: Vec<u8> = rgba(w, h, seed)
                    .chunks(4)
                    .flat_map(|p| [p[0], p[1], p[2]])
                    .collect();
                apc("a=T,f=100,i=9", &png(w, h, &rgb))
            }
            _ => apc(
                &format!("a=T,f=32,s={},v={},i=9", w.raw(), h.raw()),
                &rgba(w, h, seed),
            ),
        };
        let delete = apc("a=d,d=A", &[]);
        let mut owned = Owned::new(&ws).with_images(crate::host::IMAGE_BUDGET);
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
        t.set_page_release(release().expect("a release"));
        feed(&mut t, b"idle\r\n");
        // **The payload is read before idle is taken**: built before it,
        // 8.4 MB of it was measured not resident until read, which the
        // first cycle would otherwise charge the release.
        let read: u64 = show.iter().chain(&delete).map(|&b| u64::from(b)).sum();
        assert!(read > 0);
        for _ in 0..3 {
            let idle = resident();
            feed(&mut t, &show);
            assert_eq!(t.images().placements().count(), 1, "shown");
            let shown = resident();
            feed(&mut t, &delete);
            assert_eq!(t.images().placements().count(), 0, "deleted");
            let after = resident();
            println!("rss {} {} {}", idle.0, shown.0, after.0);
        }
    }

    /// [`rss_child`]'s cycles that end in `RIS`: `kind` is `tiles`,
    /// `sixel-refused` or `sixel-moved`, with `,nolend` for the run with
    /// no release lent.
    fn rss_child_reset(kind: &str, ws: WinSize) {
        let lend = !kind.ends_with(",nolend");
        let dot = apc("a=t,f=32,s=1,v=1,i=1", &[0x11, 0x22, 0x33, 0xff]);
        let dots = |n: u32| -> Vec<u8> {
            (1..=n)
                .flat_map(|i| {
                    apc(
                        &format!("a=t,f=32,s=1,v=1,i={i}"),
                        &[0x11, 0x22, 0x33, 0xff],
                    )
                })
                .collect()
        };
        let (w, h) = (ImagePx::of(1920), ImagePx::of(1080));
        let show = match kind.split(',').next() {
            Some("tiles") => {
                let tile = rgba(ImagePx::of(63), ImagePx::of(65), Chan::from_u8(0x5c));
                (1..=32)
                    .flat_map(|i| apc(&format!("a=t,f=32,s=63,v=65,i={i}"), &tile))
                    .collect()
            }
            Some("sixel-refused") => {
                let mut v = dots(32);
                v.extend(green_sixel(w, h));
                v
            }
            _ => {
                let mut v = dot.clone();
                v.extend(apc(
                    &format!("a=t,f=32,s={},v={},i=2", w.raw(), h.raw()),
                    &rgba(w, h, Chan::from_u8(0x42)),
                ));
                v.extend(apc("a=t,f=32,s=1,v=1,i=3", &[0x11, 0x22, 0x33, 0xff]));
                v.extend(apc("a=d,d=I,i=2", &[]));
                v.extend(green_sixel(w, h));
                v
            }
        };
        let mut owned = Owned::new(&ws).with_images(crate::host::IMAGE_BUDGET);
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
        if lend {
            t.set_page_release(release().expect("a release"));
        }
        feed(&mut t, b"idle\r\n");
        let read: u64 = show.iter().map(|&b| u64::from(b)).sum();
        assert!(read > 0);
        for _ in 0..3 {
            let idle = resident();
            feed(&mut t, &show);
            let shown = resident();
            feed(&mut t, b"\x1bc");
            assert_eq!(t.images().placements().count(), 0, "reset");
            let after = resident();
            println!("rss {} {} {}", idle.0, shown.0, after.0);
        }
    }
}
