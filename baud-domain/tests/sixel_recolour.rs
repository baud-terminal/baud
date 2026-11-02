//! **A drawn Sixel changes colour when its register is redefined**, as a
//! VT340's one colour map makes it: "You can draw an image, then change its
//! appearance by changing the associated output map location ... you do
//! not have to redraw an image to change its color"
//! (`[[source: vt3xx-screen-control]]`, "Output Mapping"). Inside one body
//! under either register mode, and across bodies under shared registers
//! (`?1070` reset), where the picture keeps a byte a pixel for it.

use baud_domain::*;

fn rgb(r: u8, g: u8, b: u8) -> Pixel {
    Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

const RED: (u8, u8, u8) = (255, 0, 0);
const GREEN: (u8, u8, u8) = (0, 255, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
/// What an undefined register paints.
const INK: (u8, u8, u8) = (10, 20, 30);

fn px((r, g, b): (u8, u8, u8)) -> Pixel {
    rgb(r, g, b)
}

/// The caller memory of a store: `words` of arena, four slots.
struct Arena {
    px: Vec<Pixel>,
    slots: [ImageSlot; 4],
    places: [PlacementSlot; 4],
    index: Vec<PlacementId>,
}

impl Arena {
    fn new(words: usize) -> Arena {
        Arena {
            px: vec![Pixel::CLEAR; words],
            slots: [ImageSlot::EMPTY; 4],
            places: [PlacementSlot::EMPTY; 4],
            index: vec![PlacementId::NONE; PlacementId::index_len(4)],
        }
    }

    fn store(&mut self) -> ImageStore<'_> {
        ImageStore::new(
            &mut self.px,
            &mut self.slots,
            &mut self.places,
            &mut self.index,
        )
    }
}

/// One Sixel at 1:1 under `registers`, undefined registers painting
/// [`INK`]: its id, or `None` where it was refused.
fn sixel(
    s: &mut SixelStream,
    store: &mut ImageStore,
    registers: SixelRegisters,
    fill: SixelFill,
    body: &[u8],
) -> Option<ImageId> {
    inked(s, store, px(INK), registers, fill, body)
}

/// [`sixel`] with undefined registers painting `ink`.
fn inked(
    s: &mut SixelStream,
    store: &mut ImageStore,
    ink: Pixel,
    registers: SixelRegisters,
    fill: SixelFill,
    body: &[u8],
) -> Option<ImageId> {
    begin(s, store, ink, registers, fill);
    for &b in body {
        s.put(StreamByte::from_stream(b), store);
    }
    s.finish(store).map(|(id, _, _)| id)
}

/// Begin a Sixel at 1:1 under `registers`, undefined registers painting
/// `ink`.
fn begin(
    s: &mut SixelStream,
    store: &mut ImageStore,
    ink: Pixel,
    registers: SixelRegisters,
    fill: SixelFill,
) {
    s.begin_with(
        SixelBegin {
            aspect: SixelAspect::ONE_TO_ONE,
            ink,
            registers,
            floor: SixelEdge::Open,
            right: SixelEdge::Open,
            fill,
        },
        store,
    );
}

fn shared(s: &mut SixelStream, store: &mut ImageStore, body: &[u8]) -> Option<ImageId> {
    sixel(s, store, SixelRegisters::Shared, SixelFill::Keep, body)
}

fn private(s: &mut SixelStream, store: &mut ImageStore, body: &[u8]) -> Option<ImageId> {
    sixel(s, store, SixelRegisters::Private, SixelFill::Keep, body)
}

/// Row 0 of image `id`.
fn row0(store: &ImageStore, id: ImageId) -> Vec<Pixel> {
    let v = store.image(id).expect("live");
    v.pixels[..v.width.index()].to_vec()
}

/// **A redefinition inside a body recolours the pixels drawn before it**,
/// under either register mode: register 1 is red for the first column and
/// blue for the second, and both end blue; register 2's green column is
/// not touched. Every row of each column alike.
#[test]
fn a_redefinition_inside_a_body_recolours_the_pixels_drawn_before_it() {
    let body = b"#1;2;100;0;0#1~#1;2;0;0;100~#2;2;0;100;0~";
    for registers in [SixelRegisters::Private, SixelRegisters::Shared] {
        let mut a = Arena::new(256);
        let mut store = a.store();
        let mut s = Box::new(SixelStream::NEW);
        let id = sixel(&mut s, &mut store, registers, SixelFill::Keep, body).expect("drawn");
        let v = store.image(id).expect("live");
        assert_eq!((v.width.raw(), v.height.raw()), (3, 6));
        for y in 0..6 {
            assert_eq!(
                &v.pixels[y * 3..y * 3 + 3],
                &[px(BLUE), px(BLUE), px(GREEN)],
                "{registers:?}, row {y}"
            );
        }
    }
}

/// **A later shared Sixel recolours the shared pictures drawn with a
/// register it redefines**: picture A draws register 1 red and register 2,
/// undefined, in the ink; picture B defines register 1 green. A's first
/// column is green and its second keeps the ink, since nothing redefined
/// register 2.
#[test]
fn a_later_shared_sixel_recolours_the_pictures_drawn_with_its_register() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let first = shared(&mut s, &mut store, b"#1;2;100;0;0#1~#2~").expect("A");
    assert_eq!(row0(&store, first), [px(RED), px(INK)], "the control");
    let second = shared(&mut s, &mut store, b"#1;2;0;100;0#1~~").expect("B");
    assert_eq!(row0(&store, first), [px(GREEN), px(INK)], "A recoloured");
    assert_eq!(row0(&store, second), [px(GREEN), px(GREEN)]);
}

/// **A palette alone recolours them, and moves the epoch**: a shared body
/// with no data is refused for having no pixels and still defines its
/// registers, so A's register 1 turns blue and the epoch moves, which is
/// what every renderer's copy of A is keyed on. A definition that keeps
/// the colour, and one of a register A never drew with, change nothing and
/// leave the epoch where it was.
#[test]
fn a_palette_alone_recolours_and_moves_the_epoch() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let first = shared(&mut s, &mut store, b"#1;2;100;0;0#1~#2~").expect("A");
    for (body, want, moves) in [
        (&b"#1;2;100;0;0"[..], [px(RED), px(INK)], false),
        (b"#9;2;0;0;100", [px(RED), px(INK)], false),
        (b"#1;2;0;0;100", [px(BLUE), px(INK)], true),
    ] {
        let epoch = store.generation();
        let empty = store.refusals.empty;
        assert_eq!(shared(&mut s, &mut store, body), None, "no pixels");
        assert_eq!(store.refusals.empty, empty + 1, "refused and counted");
        assert_eq!(row0(&store, first), want);
        assert_eq!(store.generation() != epoch, moves, "{body:?}: the epoch");
    }
}

/// **A private Sixel neither recolours nor is recoloured**: its registers
/// end with it. A shared picture keeps its red when a private Sixel
/// defines register 1 green, and a private picture keeps its red when a
/// shared one does.
#[test]
fn a_private_sixel_neither_recolours_nor_is_recoloured() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let kept = shared(&mut s, &mut store, b"#1;2;100;0;0#1~").expect("shared");
    private(&mut s, &mut store, b"#1;2;0;100;0#1~").expect("private");
    assert_eq!(
        row0(&store, kept),
        [px(RED)],
        "a private one recolours none"
    );
    let mut a = Arena::new(256);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let mine = private(&mut s, &mut store, b"#1;2;100;0;0#1~").expect("private");
    shared(&mut s, &mut store, b"#1;2;0;100;0#1~").expect("shared");
    assert_eq!(row0(&store, mine), [px(RED)], "a private one keeps its own");
    assert_eq!(store.registers(mine), None, "and keeps no register numbers");
}

/// **A shared picture keeps a byte a pixel after its pixels, a private
/// one nothing**: an 8x6 Sixel is 48 words of pixels and, shared, 12 more
/// for its register numbers, which are the registers it drew with. An
/// arena of 108 words holds one of each; a second shared one is refused
/// for the arena where a second private one would fit.
#[test]
fn a_shared_picture_keeps_a_byte_a_pixel_and_a_private_one_nothing() {
    let body = b"#1;2;100;0;0#1!4~#3;2;0;0;100#3!4~";
    let mut a = Arena::new(108);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let one = shared(&mut s, &mut store, body).expect("shared");
    let numbers = store.registers(one).expect("kept");
    assert_eq!(numbers.len(), 48);
    for y in 0..6 {
        assert_eq!(
            &numbers[y * 8..y * 8 + 8],
            &[1, 1, 1, 1, 3, 3, 3, 3],
            "row {y}"
        );
    }
    let two = private(&mut s, &mut store, body).expect("private beside it");
    assert_eq!(store.registers(two), None);
    assert_eq!(store.refusals, ImageRefusals::NONE);
    store.free(two);
    assert_eq!(shared(&mut s, &mut store, body), None, "60 + 60 > 108");
    assert_eq!(store.refusals.arena, 1, "refused for the arena");
    assert!(private(&mut s, &mut store, body).is_some(), "60 + 48 fits");
}

/// **A shared picture with room for its pixels and not its register
/// numbers is refused whole**, at its size, so no picture is kept that a
/// later redefinition would draw in colours its registers no longer have.
/// An arena of 59 words: a private 8x6 Sixel fits in 48 of them; a shared
/// one needs 60.
#[test]
fn a_shared_picture_without_room_for_its_numbers_is_refused() {
    let body = b"#1;2;100;0;0#1!8~";
    let mut a = Arena::new(59);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    assert_eq!(shared(&mut s, &mut store, body), None);
    assert_eq!(store.images_refused, 1);
    assert_eq!(store.refusals.arena, 1);
    let id = private(&mut s, &mut store, body).expect("the control: its pixels fit");
    assert_eq!(row0(&store, id), [px(RED); 8]);
}

/// **An erased pixel stays erased and the fill stays the fill** when the
/// picture is recoloured: a shared Sixel with a background fill (`P2` 0)
/// over 4x6, its data register 1 in columns 0 and 2, and the cell over
/// columns 2 and 3 erased by text; register 1 redefined turns column 0
/// green and leaves column 1 the fill and columns 2 and 3 clear.
#[test]
fn an_erased_pixel_and_the_fill_stay_as_they_are() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    store.set_cell_height(ImagePx::of(6));
    let mut s = Box::new(SixelStream::NEW);
    let fill = SixelFill::Background {
        w: 4,
        h: 6,
        cell: 0,
    };
    let id = sixel(
        &mut s,
        &mut store,
        SixelRegisters::Shared,
        fill,
        b"#1;2;100;0;0#1~?~",
    )
    .expect("drawn");
    let p = store.place(
        id,
        ImageScreen::Primary,
        LineIdx::of(0),
        Col::ZERO,
        ZIndex::ZERO,
    );
    assert!(!p.is_none(), "placed");
    assert_eq!(
        row0(&store, id),
        [px(RED), Pixel::FILL, px(RED), Pixel::FILL],
        "the control"
    );
    store.erase(
        ImageScreen::Primary,
        LineIdx::of(0),
        1,
        2,
        ImagePx::of(2),
        ImagePx::of(6),
    );
    assert_eq!(
        row0(&store, id),
        [px(RED), Pixel::FILL, Pixel::CLEAR, Pixel::CLEAR],
        "the cell over columns 2 and 3 erased"
    );
    assert_eq!(shared(&mut s, &mut store, b"#1;2;0;100;0"), None);
    assert_eq!(
        row0(&store, id),
        [px(GREEN), Pixel::FILL, Pixel::CLEAR, Pixel::CLEAR]
    );
}

/// **Two registers that paint one colour keep their own pixels when one of
/// them is redefined**: registers 1 and 2 are both red, then registers 3
/// and 4 both paint in the ink, undefined; redefining 1 blue and defining 3
/// green turns only their own columns, and 2 stays red and 4 the ink. A
/// pixel's colour alone could not say which register drew it.
#[test]
fn two_registers_of_one_colour_keep_their_own_pixels_when_one_changes() {
    let body = b"#1;2;100;0;0#2;2;100;0;0#1~#2~#3~#4~#1;2;0;0;100#3;2;0;100;0";
    for registers in [SixelRegisters::Private, SixelRegisters::Shared] {
        let mut a = Arena::new(256);
        let mut store = a.store();
        let mut s = Box::new(SixelStream::NEW);
        let id = sixel(&mut s, &mut store, registers, SixelFill::Keep, body).expect("drawn");
        assert_eq!(
            row0(&store, id),
            [px(BLUE), px(RED), px(GREEN), px(INK)],
            "{registers:?}"
        );
    }
}

/// **A body finished deferring leaves the stored pictures to the next
/// recolour**, which takes every body's since in one pass: picture A
/// draws register 1 red, two palettes finished deferring make it green
/// and then blue, and A keeps its red and the epoch until the recolour,
/// which turns A blue and moves the epoch once. A second recolour, with
/// nothing owed, changes nothing.
#[test]
fn a_deferred_finish_leaves_the_pictures_to_the_next_recolour() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    let mut s = Box::new(SixelStream::NEW);
    let first = shared(&mut s, &mut store, b"#1;2;100;0;0#1~").expect("A");
    let epoch = store.generation();
    for body in [&b"#1;2;0;100;0"[..], b"#1;2;0;0;100"] {
        s.begin_with(
            SixelBegin {
                aspect: SixelAspect::ONE_TO_ONE,
                ink: px(INK),
                registers: SixelRegisters::Shared,
                floor: SixelEdge::Open,
                right: SixelEdge::Open,
                fill: SixelFill::Keep,
            },
            &mut store,
        );
        for &b in body {
            s.put(StreamByte::from_stream(b), &mut store);
        }
        assert_eq!(s.finish_deferring(&mut store), None, "no pixels");
    }
    assert_eq!(row0(&store, first), [px(RED)], "not before the recolour");
    assert_eq!(store.generation(), epoch);
    s.recolour(&mut store);
    assert_eq!(row0(&store, first), [px(BLUE)], "the last colour");
    assert_eq!(store.generation().raw(), epoch.raw() + 1, "one pass");
    s.recolour(&mut store);
    assert_eq!(store.generation().raw(), epoch.raw() + 1, "nothing owed");
}

/// **A pixel drawn in a clear ink takes its register's colour once a later
/// shared Sixel defines it**: with no foreground said an undefined
/// register paints a clear ink, drawn as nothing, and the pixel is still
/// one its register drew -- registers 1 and 0 here, 0 being the register
/// yazi leaves undefined -- so a palette that defines them turns those
/// pixels blue and green, as it turns the pixels of the control, drawn in
/// a known ink. The position the data left at zero stays clear, and under
/// a fill (`P2` 0, over 6 by 6) the fill, on both.
#[test]
fn a_pixel_drawn_in_a_clear_ink_takes_its_register_once_defined() {
    let fill = SixelFill::Background {
        w: 6,
        h: 6,
        cell: 0,
    };
    let clear = Pixel::CLEAR;
    let cases = [
        (SixelFill::Keep, vec![px(BLUE), px(GREEN), clear, px(GREEN)]),
        (
            fill,
            vec![
                px(BLUE),
                px(GREEN),
                Pixel::FILL,
                px(GREEN),
                Pixel::FILL,
                Pixel::FILL,
            ],
        ),
    ];
    for (ink, what) in [
        (clear, "no foreground said"),
        (px(INK), "an ink, the control"),
    ] {
        for (fill, want) in &cases {
            let mut a = Arena::new(256);
            let mut store = a.store();
            let mut s = Box::new(SixelStream::NEW);
            let shared = SixelRegisters::Shared;
            let id = inked(&mut s, &mut store, ink, shared, *fill, b"#1~#0~?~").expect("drawn");
            let before = row0(&store, id);
            assert_eq!(
                before[0].alpha(),
                ink.alpha(),
                "{what}, {fill:?}: in the ink"
            );
            let palette = b"#0;2;0;100;0#1;2;0;0;100";
            assert_eq!(inked(&mut s, &mut store, ink, shared, *fill, palette), None);
            assert_eq!(&row0(&store, id), want, "{what}, {fill:?}");
        }
    }
}

/// **An erase keeps a picture drawn in a clear ink**, which is still
/// drawn, and its pixels outside the erase take the colour defined later:
/// four columns of register 1, undefined, with no foreground said; the
/// cell over columns 2 and 3 erased by text; register 1 defined blue turns
/// columns 0 and 1 blue and leaves 2 and 3 clear. An erase that counted
/// the clear ink as nothing drawn freed the picture whole.
#[test]
fn an_erase_keeps_a_picture_drawn_in_a_clear_ink() {
    let mut a = Arena::new(256);
    let mut store = a.store();
    store.set_cell_height(ImagePx::of(6));
    let mut s = Box::new(SixelStream::NEW);
    let (shared, keep) = (SixelRegisters::Shared, SixelFill::Keep);
    let clear = Pixel::CLEAR;
    let id = inked(&mut s, &mut store, clear, shared, keep, b"#1~~~~").expect("drawn");
    let p = store.place(
        id,
        ImageScreen::Primary,
        LineIdx::of(0),
        Col::ZERO,
        ZIndex::ZERO,
    );
    assert!(!p.is_none(), "placed");
    store.erase(
        ImageScreen::Primary,
        LineIdx::of(0),
        1,
        2,
        ImagePx::of(2),
        ImagePx::of(6),
    );
    assert!(store.image(id).is_some(), "the erase keeps the picture");
    assert_eq!(
        inked(&mut s, &mut store, clear, shared, keep, b"#1;2;0;0;100"),
        None
    );
    assert_eq!(row0(&store, id), [px(BLUE), px(BLUE), clear, clear]);
}

/// **A shared body's definitions reach the stored pictures at the
/// recolour after them, before the body ends**: picture A draws register 1
/// red; a palette body defines it green, the recolour a terminal makes at
/// a read's end turns A green while the body is still open, and its next
/// definition, blue, turns it blue at the next one, as a VT340's colour
/// map, read at each refresh, shows each definition. A definition is
/// applied when the byte after it arrives, which is the `#` and the `$`.
/// The body's end changes nothing more, and taking the recolour only at
/// the end leaves the same pixels.
#[test]
fn an_open_shared_body_gives_its_definitions_at_each_recolour() {
    let (shared, keep) = (SixelRegisters::Shared, SixelFill::Keep);
    let steps = [(&b"#1;2;0;100;0#"[..], GREEN), (&b"1;2;0;0;100$"[..], BLUE)];
    for each in [true, false] {
        let mut a = Arena::new(256);
        let mut store = a.store();
        let mut s = Box::new(SixelStream::NEW);
        let first = shared_red(&mut s, &mut store);
        begin(&mut s, &mut store, px(INK), shared, keep);
        for (bytes, colour) in steps {
            for &b in bytes {
                s.put(StreamByte::from_stream(b), &mut store);
            }
            if each {
                s.recolour(&mut store);
                assert_eq!(row0(&store, first), [px(colour)], "after {bytes:?}");
            }
        }
        assert_eq!(s.finish(&mut store), None, "a palette alone");
        assert_eq!(row0(&store, first), [px(BLUE)], "each read: {each}");
    }
}

/// **A shared body abandoned without an end keeps what it defined**, as
/// one ended does, wherever the recolours fell: register 1 defined green
/// and the body let go with no terminator, the next Sixel begun at once,
/// as a terminal begins it in the same read, before any recolour; that
/// later shared picture paints register 1, undefined in it, green, and
/// picture A turns green with it.
#[test]
fn an_abandoned_shared_body_keeps_what_it_defined() {
    let (shared, keep) = (SixelRegisters::Shared, SixelFill::Keep);
    for each in [true, false] {
        let mut a = Arena::new(256);
        let mut store = a.store();
        let mut s = Box::new(SixelStream::NEW);
        let first = shared_red(&mut s, &mut store);
        begin(&mut s, &mut store, px(INK), shared, keep);
        for &b in b"#1;2;0;100;0$" {
            s.put(StreamByte::from_stream(b), &mut store);
        }
        if each {
            s.recolour(&mut store);
        }
        s.abandon(&mut store);
        let later = shared_inked_one(&mut s, &mut store);
        assert_eq!(row0(&store, later), [px(GREEN)], "each read: {each}");
        assert_eq!(row0(&store, first), [px(GREEN)], "each read: {each}");
    }
}

/// A shared 1x6 picture of register 1, defined red.
fn shared_red(s: &mut SixelStream, store: &mut ImageStore) -> ImageId {
    shared(s, store, b"#1;2;100;0;0#1~").expect("A")
}

/// A shared 1x6 picture of register 1, not defined in it.
fn shared_inked_one(s: &mut SixelStream, store: &mut ImageStore) -> ImageId {
    shared(s, store, b"#1~").expect("a later picture")
}
