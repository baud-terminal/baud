//! **A Sixel is decoded as its body arrives, and draws what the buffered
//! decoder drew.**
//!
//! Until 2026-10-14 a Sixel's body was held whole in a scratch and decoded at
//! its terminator, and a body past the scratch was refused. Now each byte the
//! handler keeps is decoded on arrival into the gap of the image arena the
//! `q` claimed (`SixelStream`), so what limits an image is its pixels, as
//! `[[source: dec-std-070]]` 070-9 §8.1.2 has it: the raster extent "does
//! not limit the number of pixels a device may image" (its p. 9, PDF p.
//! 920). The oracle is the buffered decoder as built at 2992a8e9
//! (`sixel_stream/buffered.rs`), a second spelling of the grammar on purpose.
//!
//! Three things are held here: the store a program of Sixels and frees leaves
//! -- every image's pixels, size and offset, and every refusal -- is the
//! buffered decoder's, a free between two bytes of a body included, but for
//! a band refused before that free; a terminal fed the same bytes in any
//! reads draws the same; and a body that never reaches its terminator leaves
//! nothing.

use baud_core::*;
use baud_domain::{
    ImageAlpha, ImageBudget, ImageId, ImagePx, ImageRefusal, ImageRefusals, ImageSlot, ImageStore,
    Pixel, PlacementId, PlacementSlot, SixelAspect, SixelStream, StreamByte,
};
use proptest::prelude::*;

mod support;

#[path = "sixel_stream/buffered.rs"]
mod buffered;

/// The committed real capture: a 64x48 ImageMagick Sixel.
const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

// ---------------------------------------------------------------------------
// The store, against the buffered decoder.
// ---------------------------------------------------------------------------

/// One step of a store program: a Sixel's body at a `P1`, the image with
/// the `k`-th live id freed, or a Sixel whose body has that free between
/// two of its bytes, before the byte at the cut -- what a resize expiring an
/// image between two reads of a body does.
#[derive(Clone, Debug)]
enum Step {
    Sixel(Option<u16>, Vec<u8>),
    Free(usize),
    FreeInside(Option<u16>, Vec<u8>, prop::sample::Index, usize),
}

/// What a free inside a body did, as the buffered program saw it, so the
/// generators can be shown to reach both outcomes that matter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Inside {
    /// An image drawn that the gap at its `q` could not hold.
    Grew,
    /// An image whose bytes before the free outgrew the gap at its `q`,
    /// refused though the free left room for it.
    Lost,
    /// Neither.
    Neither,
}

/// Whether the bytes of a body before a free still fitted the gap its `q`
/// claimed, or had already outgrown it and lost a band.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prefix {
    Fitted,
    Outgrew,
}

/// Free the image with the `k`-th live id, if any is live.
fn free_kth(store: &mut ImageStore, ids: &mut Vec<ImageId>, k: usize) {
    if !ids.is_empty() {
        let id = ids.remove(k % ids.len());
        store.free(id);
    }
}

/// The largest gap between the live images of an arena of `len` words, in
/// words: what a Sixel claims at its `q`. Spelled here from the images'
/// offsets rather than asked of the store, for the oracle's sake.
fn largest_gap(store: &ImageStore, base: usize, ids: &[ImageId], len: usize) -> usize {
    let mut spans: Vec<(usize, usize)> = ids
        .iter()
        .map(|&id| {
            let v = store.image(id).expect("live");
            let at = (v.pixels.as_ptr().addr() - base) / core::mem::size_of::<Pixel>();
            (at, at + v.pixels.len())
        })
        .collect();
    spans.sort_unstable();
    let mut from = 0;
    let mut best = 0;
    for (start, end) in spans {
        best = best.max(start.saturating_sub(from));
        from = from.max(end);
    }
    best.max(len.saturating_sub(from))
}

/// What one step left: the image recorded, with its offset in the arena,
/// size and pixels, or nothing; and the refusals so far.
type After = (Option<(ImageId, usize, u16, u16, Vec<u32>)>, ImageRefusals);

/// Every live image's id and pixels after a step.
type Live = Vec<(ImageId, Vec<u32>)>;

/// A program's steps, each with what it left and what was live after it.
type Trace = Vec<(After, Live)>;

/// A store over an arena of `len` words that start as a sentinel, so a
/// pixel neither decoder cleared or painted shows; four slots, so a program
/// reaches the slot table.
struct Arena {
    px: Vec<Pixel>,
    slots: Vec<ImageSlot>,
    places: Vec<PlacementSlot>,
    index: Vec<PlacementId>,
}

impl Arena {
    fn new(len: usize) -> Arena {
        let sentinel = Pixel::opaque(
            baud_domain::Chan::from_u8(1),
            baud_domain::Chan::from_u8(2),
            baud_domain::Chan::from_u8(3),
        );
        Arena {
            px: vec![sentinel; len],
            slots: vec![ImageSlot::EMPTY; 4],
            places: vec![PlacementSlot::EMPTY; 4],
            index: vec![PlacementId::NONE; PlacementId::index_len(4)],
        }
    }
}

fn recorded(store: &ImageStore, base: usize, id: ImageId) -> (ImageId, usize, u16, u16, Vec<u32>) {
    let v = store.image(id).expect("recorded means live");
    let at = (v.pixels.as_ptr().addr() - base) / core::mem::size_of::<Pixel>();
    let px = v.pixels.iter().map(|c| c.bits()).collect();
    (id, at, v.width.raw(), v.height.raw(), px)
}

/// Every live image's id and pixels, to show a decode wrote over none.
fn live(store: &ImageStore, ids: &[ImageId]) -> Live {
    ids.iter()
        .filter_map(|&id| {
            Some((
                id,
                store.image(id)?.pixels.iter().map(|c| c.bits()).collect(),
            ))
        })
        .collect()
}

/// The program through the buffered decoder: measure, refuse in
/// `sixel_end`'s order, `store` at the lowest gap, clear, paint.
///
/// A free inside a body is taken before the measure, as though it came at
/// the `q`, with one exception the stream cannot close: when the bytes
/// before the free already reach an extent past the gap the `q` claimed,
/// the band that did not fit was refused with its bytes, so the image is
/// refused as one with no gap would be -- too large, empty, a slot, then
/// the arena at its whole size. An open departure from the buffered
/// decoder, which had every byte at its terminator.
fn through_buffered(arena: usize, prog: &[Step]) -> Trace {
    through_buffered_counting(arena, prog, &mut Vec::new())
}

/// [`through_buffered`], saying what each free inside a body did.
fn through_buffered_counting(arena: usize, prog: &[Step], inside: &mut Vec<Inside>) -> Trace {
    let mut a = Arena::new(arena);
    let base = a.px.as_ptr().addr();
    let mut store = ImageStore::new(&mut a.px, &mut a.slots, &mut a.places, &mut a.index);
    let mut ids: Vec<ImageId> = Vec::new();
    let mut out = Vec::new();
    for step in prog {
        let mut got = None;
        match step {
            Step::Free(k) => free_kth(&mut store, &mut ids, *k),
            Step::Sixel(p1, body) => {
                let id = buffered_sixel(&mut store, &mut ids, *p1, body, Prefix::Fitted);
                got = id.map(|id| recorded(&store, base, id));
            }
            Step::FreeInside(p1, body, cut, k) => {
                let cut = cut.index(body.len() + 1);
                let claim = largest_gap(&store, base, &ids, arena);
                let rows = buffered::rows_of_macro(*p1);
                let before = buffered::walk(&body[..cut], rows, None);
                let prefix = match u64::from(before.width) * u64::from(before.height) > claim as u64
                {
                    true => Prefix::Outgrew,
                    false => Prefix::Fitted,
                };
                free_kth(&mut store, &mut ids, *k);
                let r = buffered::walk(body, rows, None);
                let whole = u64::from(r.width) * u64::from(r.height);
                // Whether the free left room a measured image would take.
                let room = (1..=65535).contains(&r.width)
                    && (1..=65535).contains(&r.height)
                    && ids.len() < 4
                    && whole <= largest_gap(&store, base, &ids, arena) as u64;
                let id = buffered_sixel(&mut store, &mut ids, *p1, body, prefix);
                got = id.map(|id| recorded(&store, base, id));
                inside.push(match (got.is_some(), prefix) {
                    (true, _) if whole > claim as u64 => Inside::Grew,
                    (false, Prefix::Outgrew) if room => Inside::Lost,
                    _ => Inside::Neither,
                });
            }
        }
        out.push(((got, store.refusals), live(&store, &ids)));
    }
    out
}

/// One Sixel through the buffered decoder, the image it recorded; or, where
/// its prefix outgrew its claim, refused as one with no gap is: the slot
/// table first, then the arena at the whole size.
fn buffered_sixel(
    store: &mut ImageStore,
    ids: &mut Vec<ImageId>,
    p1: Option<u16>,
    body: &[u8],
    prefix: Prefix,
) -> Option<ImageId> {
    let rows = buffered::rows_of_macro(p1);
    let r = buffered::walk(body, rows, None);
    if r.width > 65535 || r.height > 65535 {
        store.refuse(ImageRefusal::TooLarge);
        return None;
    }
    if r.width == 0 || r.height == 0 {
        store.refuse(ImageRefusal::Empty);
        return None;
    }
    let (w, h) = (
        u16::try_from(r.width).expect("fits"),
        u16::try_from(r.height).expect("fits"),
    );
    let (iw, ih) = (ImagePx::of(w), ImagePx::of(h));
    if prefix == Prefix::Outgrew {
        // Four slots, every live one in `ids`.
        store.refuse(match ids.len() < 4 {
            true => ImageRefusal::Arena(iw, ih),
            false => ImageRefusal::Slot,
        });
        return None;
    }
    let (id, buf) = store.store(iw, ih, ImageAlpha::OpaqueOrClear)?;
    buf.fill(Pixel::CLEAR);
    let _ = buffered::walk(body, rows, Some((buf, usize::from(w))));
    ids.push(id);
    Some(id)
}

/// The same program through the stream, a byte at a time.
fn through_stream(arena: usize, prog: &[Step]) -> Trace {
    let mut a = Arena::new(arena);
    let base = a.px.as_ptr().addr();
    let mut store = ImageStore::new(&mut a.px, &mut a.slots, &mut a.places, &mut a.index);
    let mut s = Box::new(SixelStream::NEW);
    let mut ids: Vec<ImageId> = Vec::new();
    let mut out = Vec::new();
    for step in prog {
        let mut got = None;
        match step {
            Step::Free(k) => free_kth(&mut store, &mut ids, *k),
            Step::Sixel(..) | Step::FreeInside(..) => {
                let (p1, body, free) = match step {
                    Step::Sixel(p1, body) => (p1, body, None),
                    Step::FreeInside(p1, body, cut, k) => {
                        (p1, body, Some((cut.index(body.len() + 1), *k)))
                    }
                    Step::Free(_) => unreachable!(),
                };
                s.begin(SixelAspect::from_macro(*p1), &mut store);
                for (i, &b) in body.iter().enumerate() {
                    if let Some((_, k)) = free.filter(|&(cut, _)| cut == i) {
                        free_kth(&mut store, &mut ids, k);
                    }
                    s.put(StreamByte::from_stream(b), &mut store);
                }
                if let Some((_, k)) = free.filter(|&(cut, _)| cut == body.len()) {
                    free_kth(&mut store, &mut ids, k);
                }
                if let Some((id, _, _)) = s.finish(&mut store) {
                    ids.push(id);
                    got = Some(recorded(&store, base, id));
                }
            }
        }
        out.push(((got, store.refusals), live(&store, &ids)));
    }
    out
}

/// A number as a Sixel sender writes one, the edges included: none, zero,
/// small, past 070-9's 32767 repeat, and past what a `u16` holds.
fn number() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        Just(Vec::new()),
        Just(b"0".to_vec()),
        (0u32..12).prop_map(|n| n.to_string().into_bytes()),
        (0u32..400).prop_map(|n| n.to_string().into_bytes()),
        Just(b"32768".to_vec()),
        Just(b"65535".to_vec()),
        Just(b"99999".to_vec()),
    ]
}

/// One piece of a Sixel body: data, the two returns, a colour selected or
/// defined -- cut short at any parameter, in RGB, HLS or another system --
/// a repeat (with or without data after it), raster attributes before data
/// or after it, with an extent that matches, understates or overstates, and
/// bytes of no meaning in the grammar: a stray `;`, digits, and `80-FF`.
fn body_atom() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        6 => (0x3fu8..=0x7e).prop_map(|b| vec![b]),
        1 => Just(b"$".to_vec()),
        1 => Just(b"-".to_vec()),
        1 => number().prop_map(|n| [b"#".to_vec(), n].concat()),
        1 => (number(), prop::sample::select(vec![0u16, 1, 2, 2, 3]), prop::collection::vec(number(), 0..5))
            .prop_map(|(pc, pu, rest)| {
                let mut v = b"#".to_vec();
                v.extend(pc);
                v.push(b';');
                v.extend(pu.to_string().into_bytes());
                for n in rest {
                    v.push(b';');
                    v.extend(n);
                }
                v
            }),
        1 => number().prop_map(|n| [b"!".to_vec(), n].concat()),
        1 => prop::collection::vec(number(), 0..5).prop_map(|ps| {
            let mut v = b"\"".to_vec();
            for (k, n) in ps.into_iter().enumerate() {
                if k > 0 {
                    v.push(b';');
                }
                v.extend(n);
            }
            v
        }),
        1 => Just(b";".to_vec()),
        1 => number(),
        1 => (0x80u8..=0xff).prop_map(|b| vec![b]),
    ]
}

fn body() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(body_atom(), 0..40).prop_map(|v| v.concat())
}

/// A body of whole bands, one to nine, each a colour defined in RGB and
/// repeated across up to 63 columns, with any atom between them: an image
/// the size of an arena's gaps, so that what a free opens is what it needs,
/// and whose pixels are not the default, so that rows left behind show.
fn banded() -> impl Strategy<Value = Vec<u8>> {
    let band = (
        (0u8..4, 0u8..=100, 0u8..=100, 0u8..=100),
        1u16..64,
        0x3fu8..=0x7e,
        prop::collection::vec(body_atom(), 0..3),
    )
        .prop_map(|((c, r, g, b), n, d, rest)| {
            let mut v = format!("#{c};2;{r};{g};{b}!{n}").into_bytes();
            v.push(d);
            v.extend(rest.concat());
            v
        });
    prop::collection::vec(band, 1..10).prop_map(|v| v.join(&b'-'))
}

fn p1() -> impl Strategy<Value = Option<u16>> {
    prop::sample::select(vec![None, Some(0), Some(1), Some(3), Some(7), Some(9)])
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (p1(), body()).prop_map(|(p, b)| Step::Sixel(p, b)),
        1 => any::<usize>().prop_map(Step::Free),
    ]
}

/// A step of a program that frees inside bodies: a Sixel of whole bands, a
/// free, or a Sixel of whole bands with a free inside its body.
fn inside_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => (p1(), banded()).prop_map(|(p, b)| Step::Sixel(p, b)),
        1 => any::<usize>().prop_map(Step::Free),
        2 => (p1(), banded(), any::<prop::sample::Index>(), any::<usize>())
            .prop_map(|(p, b, cut, k)| Step::FreeInside(p, b, cut, k)),
    ]
}

/// An arena the size of a few banded images.
fn inside_arena() -> impl Strategy<Value = usize> {
    256usize..8192
}

/// An arena from none to one that holds several images, and sizes between
/// where an image fits only once an earlier one is freed.
fn arena() -> impl Strategy<Value = usize> {
    prop_oneof![
        Just(0usize),
        0usize..64,
        64usize..4096,
        Just(1 << 12),
        Just(1 << 16)
    ]
}

// ---------------------------------------------------------------------------
// The terminal, at every read boundary.
// ---------------------------------------------------------------------------

/// What ends a Sixel's body: its terminator, or what abandons it -- `CAN`,
/// `SUB`, an `ESC` and a final that is not `\`, `RIS`, and a new DCS.
fn ending() -> impl Strategy<Value = &'static [u8]> {
    prop::sample::select(vec![
        &b"\x1b\\"[..],
        b"\x1b\\",
        b"\x1b\\",
        b"\x18",
        b"\x1a",
        b"\x1b7",
        b"\x1bc",
        b"\x1bP$qm\x1b\\",
    ])
}

/// A byte the handler drops or folds inside a body: a C0 the graph hands a
/// DCS body (`BEL`, the format effectors), `SPACE`, `DEL`, and GR bytes,
/// which are their GL bytes under ISO 8859-1 and kept as they came under
/// UTF-8.
fn noise() -> impl Strategy<Value = Vec<u8>> {
    prop::sample::select(vec![
        vec![0x07u8],
        vec![0x08],
        vec![0x0a],
        vec![0x0d],
        vec![0x20],
        vec![0x7f],
        vec![0xa0],
        vec![0xa1],
        vec![0xa3],
        vec![0xbf],
        vec![0xfe],
        vec![0xff],
    ])
}

/// One piece of a terminal program: a Sixel with noise spliced into its
/// body, ended any way; a row of text written across the screen, which frees
/// an image it lands on; a cursor move; and the encoding switched.
fn term_atom() -> impl Strategy<Value = Vec<u8>> {
    let spliced = prop::collection::vec(prop_oneof![4 => body_atom(), 1 => noise()], 0..30)
        .prop_map(|v| v.concat());
    prop_oneof![
        4 => (p1(), prop::sample::select(vec![&b""[..], b";1", b";0", b";2"]), spliced, ending())
            .prop_map(|(p, p2, b, e)| {
                let mut v = b"\x1bP".to_vec();
                if let Some(p) = p {
                    v.extend(p.to_string().into_bytes());
                }
                v.extend_from_slice(p2);
                v.push(b'q');
                v.extend(b);
                v.extend_from_slice(e);
                v
            }),
        2 => (1u8..7).prop_map(|r| format!("\x1b[{r};1Habcdefghij klmnopqrs").into_bytes()),
        1 => (1u8..6, 1u8..20).prop_map(|(r, c)| format!("\x1b[{r};{c}H").into_bytes()),
        1 => Just(b"\x1b%@".to_vec()),
        1 => Just(b"\x1b%G".to_vec()),
    ]
}

fn term_program() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(term_atom(), 1..8).prop_map(|v| v.concat())
}

/// What a terminal shows of its images: each placement's anchor, line and
/// column, and its image's size and pixels, the refusals, the
/// opaque-background count, and the cursor.
type Shown = (
    Vec<(u64, Col, u16, u16, Vec<u32>)>,
    ImageRefusals,
    usize,
    (Row, Col),
);

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

/// A 20x6 terminal with 64 lines of history and an arena of 8 Ki words: a
/// few small images, and quick to build, which a property building three a
/// case at 20000 cases needs.
fn small() -> Owned {
    Owned::with_history(
        &WS,
        &baud_domain::CacheGeometry::UNKNOWN,
        baud_domain::Depth::of(64),
    )
    .with_images(ImageBudget::of(1 << 13))
}

/// Feed `data` as reads cut at `cuts`, ascending, into a terminal with a
/// small arena and an 8x16 cell, so text frees what it lands on.
fn shown(data: &[u8], cuts: &[usize]) -> Shown {
    let mut owned = small();
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    let mut at = 0;
    for &end in cuts.iter().chain(core::iter::once(&data.len())) {
        if end > at {
            t.write_at(BytePos::ZERO, &data[at..end]);
            t.feed(BytePos::ZERO, ReadLen::from_index(end - at));
            at = end;
        }
    }
    let im = t.images();
    let placed = im
        .placements()
        .map(|(_, p)| {
            let v = im.image(p.image).expect("placed means live");
            let px = v.pixels.iter().map(|c| c.bits()).collect();
            (p.line.value(), p.col, v.width.raw(), v.height.raw(), px)
        })
        .collect();
    (placed, im.refusals, im.sixel_p2_ignored, t.cursor())
}

/// Reads of `k` bytes each.
fn every(len: usize, k: usize) -> Vec<usize> {
    (1..len.div_ceil(k)).map(|i| i * k).collect()
}

proptest! {
    /// **A program of Sixels and frees leaves the store the buffered decoder
    /// left**: after every step, the image recorded -- its id, its offset in
    /// the arena, its size and every pixel -- every refusal by cause, and
    /// every live image's pixels, untouched by a later decode. The arena
    /// starts as a sentinel, so a word the stream neither cleared nor
    /// painted shows; four slots and arenas from none to 64 Ki words reach
    /// every refusal and every gap the frees open.
    #[test]
    fn a_program_of_sixels_leaves_the_store_the_buffered_decoder_left(
        len in arena(),
        prog in prop::collection::vec(step(), 1..10),
    ) {
        prop_assert_eq!(through_stream(len, &prog), through_buffered(len, &prog));
    }

    /// **A free between two bytes of a body is room the rest of it can
    /// use**: programs of Sixels of whole bands, frees, and Sixels with a
    /// free at any point in their bodies, over arenas of 256 to 8191
    /// words, leave the store the buffered decoder left with each free
    /// taken at its `q` -- but where the bytes before the free already
    /// outgrew the gap the `q` claimed, which the buffered program refuses
    /// as an image with no gap is (`through_buffered`).
    #[test]
    fn a_free_inside_a_body_leaves_the_store_the_buffered_decoder_left(
        len in inside_arena(),
        prog in prop::collection::vec(inside_step(), 1..10),
    ) {
        prop_assert_eq!(through_stream(len, &prog), through_buffered(len, &prog));
    }

    /// **Where the reads fall does not change what a terminal shows**: a
    /// program of Sixels -- noise spliced into their bodies, abandoned by
    /// every ending there is, under either encoding -- and the text that
    /// frees them, fed whole, in reads of `k` bytes, and as two reads split
    /// at `at`, places the same images with the same pixels and refuses the
    /// same.
    #[test]
    fn a_sixel_program_draws_alike_at_every_read_boundary(
        prog in term_program(),
        k in 1usize..64,
        at in any::<prop::sample::Index>(),
    ) {
        let whole = shown(&prog, &[]);
        prop_assert_eq!(&shown(&prog, &every(prog.len(), k)), &whole, "reads of {}", k);
        let at = at.index(prog.len() + 1);
        prop_assert_eq!(&shown(&prog, &[at]), &whole, "split at {}", at);
    }
}

// ---------------------------------------------------------------------------
// Deterministic cases.
// ---------------------------------------------------------------------------

/// The swirl's image and refusals alone in a fresh terminal: the control.
fn swirl_alone() -> Shown {
    shown_at(SWIRL, ImageBudget::of(64 * 48 + 64))
}

fn shown_at(data: &[u8], budget: ImageBudget) -> Shown {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(budget);
    let mut t = Term::new(owned.storage(), &ws);
    for read in data.chunks(READ_CHUNK) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
    let im = t.images();
    let placed = im
        .placements()
        .map(|(_, p)| {
            let v = im.image(p.image).expect("live");
            (
                p.line.value(),
                p.col,
                v.width.raw(),
                v.height.raw(),
                v.pixels.iter().map(|c| c.bits()).collect(),
            )
        })
        .collect();
    (placed, im.refusals, im.sixel_p2_ignored, t.cursor())
}

/// **A body cut off is placed from what it received**: four fifths of the
/// swirl's body, past its colour definitions and into its data, then what
/// cuts it off -- `CAN`, an `ESC` and a final that is not `\`, a new DCS
/// -- leaves what the same four fifths ended by `ST` and followed by the
/// cut's own escape sequence leaves, then the whole swirl at home: williams'
/// "Cancelling a control string does not undo these effects" (501-503) and
/// 070-9's printing in received order (Jev, KEEP, 0.94/0.94; the lean KEEP,
/// 0.72/0.68, for `CAN`). Until 2026-10-19 each left nothing. `RIS` still
/// leaves nothing: it places the image and then removes every one, and
/// done forty times, past the 32 image slots, the swirl after it still
/// draws alone, so no cut keeps a slot or a span.
#[test]
fn a_body_cut_off_is_placed_from_what_it_received() {
    let want = swirl_alone();
    assert_eq!(want.0.len(), 1, "the control draws one image");
    assert_eq!(want.1, ImageRefusals::NONE, "and refuses nothing");
    let part = &SWIRL[..SWIRL.len() * 4 / 5];
    let budget = ImageBudget::of(64 * 48 + 64);
    for (name, cut, then) in [
        ("CAN", &b"\x18"[..], &b""[..]),
        ("ESC then a final", b"\x1b7", b"\x1b7"),
        ("a new DCS", b"\x1bP$qm\x1b\\", b"\x1bP$qm\x1b\\"),
    ] {
        let cut_off = shown_at(&[part, cut].concat(), budget);
        let ended = shown_at(&[part, b"\x1b\\", then].concat(), budget);
        assert_eq!(cut_off, ended, "{name}: as if ST had come first");
        assert_eq!(cut_off.0.len(), 1, "{name}: the part is placed");
        let got = shown_at(&[part, cut, b"\x1b[H", SWIRL].concat(), budget);
        let ended = shown_at(&[part, b"\x1b\\", then, b"\x1b[H", SWIRL].concat(), budget);
        assert_eq!(got, ended, "{name}: and the swirl after it");
    }
    for times in [1, 40] {
        let mut data = Vec::new();
        for _ in 0..times {
            data.extend_from_slice(part);
            data.extend_from_slice(b"\x1bc");
        }
        data.extend_from_slice(b"\x1b[H");
        data.extend_from_slice(SWIRL);
        let got = shown_at(&data, budget);
        assert_eq!(got.0, want.0, "RIS x{times}: the image as alone");
        assert_eq!(
            got.1,
            ImageRefusals::NONE,
            "RIS x{times}: refused for nothing"
        );
    }
}

/// **The raster's extent is a summary, not a limit**: one that overstates
/// the image, understates it, is zero or is absent draws the data's extent,
/// 30x12 here, with every pixel the data set and no other. 070-9 §8.1.2:
/// "This parameter does not limit the number of pixels a device may image"
/// and "the receiving device will NOT clip the image" (its pp. 9-10, PDF pp.
/// 920-921).
#[test]
fn a_raster_that_overstates_or_understates_the_image_draws_the_datas_extent() {
    let red = Pixel::opaque(
        baud_domain::Chan::from_u8(255),
        baud_domain::Chan::from_u8(0),
        baud_domain::Chan::from_u8(0),
    )
    .bits();
    for raster in [
        &b"\"1;1;65535;65535"[..],
        b"\"1;1;30;12",
        b"\"1;1;1;1",
        b"\"1;1;0;0",
        b"",
    ] {
        let mut d = b"\x1bP9;1q".to_vec();
        d.extend_from_slice(raster);
        d.extend_from_slice(b"#1;2;100;0;0#1!20~-!30~\x1b\\");
        let name = String::from_utf8_lossy(raster).into_owned();
        let (placed, refused, _, _) = shown_at(&d, ImageBudget::of(1 << 16));
        assert_eq!(refused, ImageRefusals::NONE, "{name}");
        let [(_, _, w, h, px)] = &placed[..] else {
            panic!("{name}: one image, got {}", placed.len());
        };
        assert_eq!((*w, *h), (30, 12), "{name}: the data's extent");
        for y in 0..12 {
            for x in 0..30 {
                let want = if y >= 6 || x < 20 { red } else { 0 };
                assert_eq!(px[y * 30 + x], want, "{name}: ({x}, {y})");
            }
        }
    }
}

/// **A body past any scratch the app had draws**: the swirl with 8 MiB of
/// graphics carriage returns before its terminator, each a no-op at the
/// left edge, draws the swirl's image exactly. At 2992a8e9 the app's 3 MiB
/// scratch refused it for its body (the record, "A Sixel is decoded as its
/// body arrives").
#[test]
fn a_body_past_any_old_scratch_draws_its_image() {
    let st = SWIRL
        .windows(2)
        .rposition(|w| w == b"\x1b\\")
        .expect("terminated");
    let mut d = SWIRL[..st].to_vec();
    d.extend(core::iter::repeat_n(b'$', 8 << 20));
    d.extend_from_slice(&SWIRL[st..]);
    let got = shown_at(&d, ImageBudget::of(64 * 48 + 64));
    assert_eq!(got.0, swirl_alone().0, "the swirl, pixel for pixel");
    assert_eq!(got.1, ImageRefusals::NONE);
}

/// **An image past the arena is refused with its whole size, however late
/// in its body it ran out**: the swirl into an arena one band short walks
/// on past the band that did not fit and is refused for the arena at 64x48,
/// not at the size it had reached.
#[test]
fn an_image_that_outgrows_its_claim_is_refused_at_its_whole_size() {
    let got = shown_at(SWIRL, ImageBudget::of(64 * 42));
    assert!(got.0.is_empty(), "nothing placed");
    assert_eq!(
        (
            got.1.arena,
            got.1.arena_width.raw(),
            got.1.arena_height.raw()
        ),
        (1, 64, 48)
    );
    assert_eq!(got.1.total(), 1, "and nothing else: {:?}", got.1);
}

/// A Sixel `w` wide and 60 tall in one colour, `rgb` its percentages,
/// unterminated: ten bands, each every bit of `w` columns.
fn ten_bands(w: usize, rgb: &str) -> Vec<u8> {
    let mut v = format!("\x1bP9;1q#1;2;{rgb}#1").into_bytes();
    for band in 0..10 {
        if band > 0 {
            v.push(b'-');
        }
        v.extend(format!("!{w}~").into_bytes());
    }
    v
}

/// **A free between two reads of a body is room the rest of it can use.**
/// On a 12x20 alternate screen with an 8x16 cell and a 3000-word arena, two
/// blue 20x60 images at rows 1 and 6 leave a 600-word gap. A red 30x60
/// Sixel, 1800 words, is begun at row 1 and the screen resized to 5 rows
/// between two reads of its body, which expires the image at row 6 and
/// opens an 1800-word gap over its span; then the rest of the body and its
/// `ST`. At every cut, fed in whole reads and a byte at a time:
///
/// * where the bytes before the resize reach at most the third band, 540
///   words, the image is drawn at row 1, every pixel red -- the rows painted
///   before the resize moved into the larger gap, none of the freed blue
///   showing -- and refused for nothing, as 2992a8e9's buffered decoder,
///   which measured at the terminator, drew it;
/// * where they reach the fourth band, 720 words painted into a 600-word
///   claim, the band that did not fit was refused with its bytes, and the
///   image is refused for the arena at 30x60: the open departure from the
///   buffered decoder (the record, "A Sixel is decoded as its body
///   arrives").
#[test]
fn a_resize_inside_a_body_frees_room_the_rest_of_it_can_use() {
    let red = Pixel::opaque(
        baud_domain::Chan::from_u8(255),
        baud_domain::Chan::from_u8(0),
        baud_domain::Chan::from_u8(0),
    )
    .bits();
    let mut first = ten_bands(20, "0;0;100");
    first.extend_from_slice(b"\x1b\\");
    let body = ten_bands(30, "100;0;0");
    let fourth = body
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == b'~')
        .nth(3)
        .expect("ten bands")
        .0;
    let ws = WinSize {
        ws_row: 12,
        ws_col: 20,
    };
    for read in [READ_CHUNK, 1] {
        for cut in 0..=body.len() {
            let mut owned = Owned::new(&ws).with_images(ImageBudget::of(3000));
            let mut t = Term::new(owned.storage(), &ws);
            t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
            let feed = |t: &mut Term, d: &[u8]| {
                for c in d.chunks(read) {
                    t.write_at(BytePos::ZERO, c);
                    t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
                }
            };
            feed(&mut t, b"\x1b[?1049h\x1b[1;1H");
            feed(&mut t, &first);
            feed(&mut t, b"\x1b[6;1H");
            feed(&mut t, &first);
            feed(&mut t, b"\x1b[1;1H");
            assert_eq!(t.images().placements().count(), 2, "the control: two");
            feed(&mut t, &body[..cut]);
            let dropped = t.images().images_dropped;
            let _ = t.resize(&WinSize {
                ws_row: 5,
                ws_col: 20,
            });
            assert_eq!(
                t.images().images_dropped,
                dropped + 1,
                "cut {cut}: the resize expires the image at row 6"
            );
            feed(&mut t, &body[cut..]);
            feed(&mut t, b"\x1b\\");
            let im = t.images();
            let drawn: Vec<_> = im
                .placements()
                .filter_map(|(_, p)| {
                    let v = im.image(p.image).expect("placed means live");
                    (v.width.raw() == 30).then(|| {
                        let px: Vec<u32> = v.pixels.iter().map(|c| c.bits()).collect();
                        (p.line.value(), p.col, v.height.raw(), px)
                    })
                })
                .collect();
            if cut <= fourth {
                assert_eq!(
                    im.refusals,
                    ImageRefusals::NONE,
                    "cut {cut}, reads of {read}"
                );
                let [(line, col, h, px)] = &drawn[..] else {
                    panic!(
                        "cut {cut}, reads of {read}: one 30-wide image, got {}",
                        drawn.len()
                    );
                };
                assert_eq!(
                    (*line, *col, *h),
                    (0, Col::ZERO, 60),
                    "cut {cut}, reads of {read}"
                );
                assert!(px.iter().all(|&c| c == red), "cut {cut}: every pixel red");
            } else {
                assert!(drawn.is_empty(), "cut {cut}, reads of {read}: refused");
                assert_eq!(
                    (
                        im.refusals.arena,
                        im.refusals.arena_width.raw(),
                        im.refusals.arena_height.raw(),
                        im.refusals.total()
                    ),
                    (1, 30, 60, 1),
                    "cut {cut}, reads of {read}"
                );
            }
        }
    }
}

/// **Every read size and every two-read split of a short program that
/// abandons a body and draws two images gives one picture**, enumerated
/// rather than sampled: the swirl cut by `CAN`, a 30x12 image, text over
/// the first, and the swirl again.
#[test]
fn every_read_size_and_every_split_of_an_abandoning_program_draw_alike() {
    let mut prog = SWIRL[..1500].to_vec();
    prog.extend_from_slice(b"\x18\x1b[2;3H\x1bP9;1q\"1;1;30;12#1;2;100;0;0#1!20~-!30~\x1b\\");
    prog.extend_from_slice(b"\x1b[1;1Habcdef\x1b[3;1H");
    prog.extend_from_slice(SWIRL);
    let whole = shown(&prog, &[]);
    assert_eq!(whole.0.len(), 2, "the control: two images placed");
    for k in 1..=prog.len() {
        assert_eq!(shown(&prog, &every(prog.len(), k)), whole, "reads of {k}");
    }
    for at in 0..=prog.len() {
        assert_eq!(shown(&prog, &[at]), whole, "split at {at}");
    }
}

/// **The generators reach what the properties claim to hold**, or a property
/// that passes says less than it reads: over 500 programs drawn the way the
/// store property draws them, the buffered decoder draws images and refuses
/// for every cause -- too large, empty, the arena and the slot table -- and
/// places an image in a gap below a live one, which only a free opens; over
/// 500 programs that free inside bodies, some draw an image the gap at its
/// `q` could not hold, and some lose a band before the free to an image the
/// free left room for; and over 1000 terminal programs some place images,
/// some have them freed by text, and some refuse. The runner is
/// deterministic, so the counts are the same on every run, and each floor
/// sits under the count taken: 821 drawn, 25 below a live image, 312 too
/// large, 121 empty, 673 for the arena and 49 for the slots; 71 drawn past
/// the gap at the `q` and 31 lost before the free; 280 programs placing, 36
/// freeing and 149 refusing.
#[test]
fn the_generators_reach_every_outcome() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let (mut drawn, mut below) = (0, 0);
    let mut refused = ImageRefusals::NONE;
    let strategy = (arena(), prop::collection::vec(step(), 1..10));
    for _ in 0..500 {
        let (len, prog) = strategy.new_tree(&mut runner).expect("a program").current();
        let out = through_buffered(len, &prog);
        // Each recorded image's id and where its span starts.
        let mut spans: Vec<(ImageId, usize)> = Vec::new();
        for ((got, _), live) in &out {
            if let Some((id, at, _, _, _)) = got {
                drawn += 1;
                let over = live
                    .iter()
                    .filter(|(l, _)| l != id)
                    .filter_map(|(l, _)| spans.iter().find(|(s, _)| s == l))
                    .any(|&(_, start)| start > *at);
                below += usize::from(over);
                spans.retain(|(s, _)| s != id);
                spans.push((*id, *at));
            }
        }
        let r = out.last().expect("one step").0.1;
        refused.too_large += r.too_large;
        refused.empty += r.empty;
        refused.arena += r.arena;
        refused.slot += r.slot;
    }
    assert!(drawn > 800, "drawn {drawn}");
    assert!(below > 20, "an image placed below a live one: {below}");
    for (name, n) in [
        ("too large", refused.too_large),
        ("empty", refused.empty),
        ("arena", refused.arena),
        ("slot", refused.slot),
    ] {
        assert!(n > 40, "{name} refused too seldom: {refused:?}");
    }
    let mut inside = Vec::new();
    let mut runner = TestRunner::deterministic();
    let strategy = (inside_arena(), prop::collection::vec(inside_step(), 1..10));
    for _ in 0..500 {
        let (len, prog) = strategy.new_tree(&mut runner).expect("a program").current();
        let _ = through_buffered_counting(len, &prog, &mut inside);
    }
    let grew = inside.iter().filter(|&&i| i == Inside::Grew).count();
    let lost = inside.iter().filter(|&&i| i == Inside::Lost).count();
    assert!(
        grew > 60 && lost > 25,
        "a free inside a body: grew {grew}, lost {lost}"
    );
    let (mut placed, mut freed, mut refusing) = (0, 0, 0);
    let mut runner = TestRunner::deterministic();
    for _ in 0..1000 {
        let prog = term_program()
            .new_tree(&mut runner)
            .expect("a program")
            .current();
        let mut owned = small();
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        for read in prog.chunks(READ_CHUNK) {
            t.write_at(BytePos::ZERO, read);
            t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        }
        let im = t.images();
        placed += usize::from(im.placements().count() > 0);
        freed += usize::from(im.images_dropped > 0);
        refusing += usize::from(im.images_refused > 0);
    }
    assert!(
        placed > 250 && freed > 30 && refusing > 140,
        "placed {placed}, freed {freed}, refusing {refusing}"
    );
}
