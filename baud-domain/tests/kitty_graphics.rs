//! The kitty graphics control data, read as the "Control data reference"
//! table gives it (`[[source: kitty-graphics]]`): every key, its default
//! where absent, and what makes a command malformed.

use baud_domain::*;

#[test]
fn every_key_is_read_and_every_absent_key_takes_its_default() {
    let c = KittyCommand::parse(b"");
    assert_eq!(c, KittyCommand::DEFAULT);
    assert_eq!(c.act(), KittyAction::Transmit, "a=t is the default");
    assert_eq!(c.format, KittyFormat::Rgba, "f=32 is the default");
    assert_eq!(c.medium, KittyMedium::Direct, "t=d is the default");
    let c = KittyCommand::parse(
        b"a=T,q=2,f=100,t=d,s=10,v=20,S=30,O=40,i=5,p=6,o=z,m=1,x=1,y=2,w=3,h=4,\
          X=5,Y=6,c=7,r=8,C=1,U=1,z=-1073741825,P=9,Q=10,H=-1,V=2,N=1,d=Z",
    );
    assert_eq!(c.error, None, "{c:?}");
    assert_eq!(c.act(), KittyAction::TransmitPut);
    assert_eq!(c.quiet, KittyQuiet::Silent);
    assert_eq!(c.format, KittyFormat::Png);
    assert_eq!((c.width, c.height, c.size, c.offset), (10, 20, 30, 40));
    assert_eq!((c.id, c.placement), (KittyId::of(5), KittyPlacement::of(6)));
    assert_eq!(
        (c.compression, c.more),
        (KittyCompression::Zlib, KittyMore::More)
    );
    assert_eq!((c.x, c.y, c.w, c.h, c.cell_x, c.cell_y), (1, 2, 3, 4, 5, 6));
    assert_eq!((c.cols, c.rows, c.cursor), (7, 8, KittyCursor::Stay));
    assert_eq!((c.virtual_place, c.z), (1, -1_073_741_825));
    assert_eq!((c.parent, c.parent_placement), (9, 10));
    assert_eq!(c.delete, (KittyDelete::Z, KittyFree::Data));
    let c = KittyCommand::parse(b"I=7,d=r");
    assert_eq!(c.number, KittyNumber::of(7));
    assert_eq!(c.delete, (KittyDelete::Range, KittyFree::Placements));
}

#[test]
fn a_malformed_key_is_an_error_and_an_unknown_one_is_not() {
    for bad in [
        &b"a=x"[..],
        b"f=25",
        b"t=x",
        b"o=y",
        b"d=k",
        b"s=4294967296",
        b"z=2147483648",
        b"s=-1",
        b"s",
        b"i=1,I=2",
    ] {
        let c = KittyCommand::parse(bad);
        assert!(c.error.is_some(), "{}", String::from_utf8_lossy(bad));
    }
    let c = KittyCommand::parse(b",,K=whatever,i=3,");
    assert_eq!(
        c.error, None,
        "empty entries and an unknown key are ignored"
    );
    assert_eq!(c.id, KittyId::of(3), "and the rest is read");
    let c = KittyCommand::parse(b"s=4294967295,z=-2147483648");
    assert_eq!(
        (c.width, c.z, c.error),
        (u32::MAX, i32::MIN, None),
        "32 bits"
    );
    // The ids survive a malformed key, so the error names the image.
    let c = KittyCommand::parse(b"f=7,i=9");
    assert_eq!((c.error, c.id), (Some(KittyError::FORMAT), KittyId::of(9)));
}

/// One `APC G` body through `k`, as the core routes it: `begin`, each
/// byte, then the terminator.
fn command(k: &mut KittyStream, store: &mut ImageStore, body: &[u8]) {
    let at = KittyAt {
        screen: ImageScreen::Primary,
        top: LineIdx::FIRST,
        row: Rows::ZERO,
        col: Col::ZERO,
        rows: Rows::of_count(24),
        cols: Cols::of_index(80),
        cell_w: ImagePx::of(10),
        cell_h: ImagePx::of(20),
    };
    let mut scratch = [0u8; 0];
    k.begin(store, &mut scratch);
    for &b in body {
        k.put(b, store, &mut scratch);
    }
    k.end(&at, store, &mut scratch);
}

/// **A reply with no room in the outbox is dropped whole and counted**,
/// never cut: an embedder that does not drain the replies between
/// commands. Five ENOENT replies of 56 bytes against 256 bytes of outbox:
/// four fit, the fifth is counted.
#[test]
fn a_reply_with_no_room_is_dropped_whole_and_counted() {
    let mut k = KittyStream::NEW;
    let (mut px, mut slots, mut places) = (
        [Pixel::CLEAR; 0],
        [ImageSlot::EMPTY; 4],
        [PlacementSlot::EMPTY; 4],
    );
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let one = b"\x1b_Gi=1;ENOENT:Put command refers to non-existent image\x1b\\";
    for _ in 0..4 {
        command(&mut k, &mut store, b"a=p,i=1");
    }
    assert_eq!(k.replies(), one.repeat(4), "four whole replies");
    assert_eq!(k.counts.replies_dropped, 0);
    command(&mut k, &mut store, b"a=p,i=1");
    assert_eq!(k.replies(), one.repeat(4), "the fifth is not cut in");
    assert_eq!(k.counts.replies_dropped, 1, "and is counted");
    k.clear_replies();
    command(&mut k, &mut store, b"a=p,i=1");
    assert_eq!(k.replies(), one, "drained, the next fits");
    assert_eq!(k.counts.replies_dropped, 1);
}

/// A reader that has nothing: every read fails.
struct Nothing;

impl KittyFiles for Nothing {
    fn read(
        &mut self,
        _: &KittyFileRequest<'_>,
        _: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        Err(KittyFileFailed)
    }
}

/// **A reader lent serves the file mediums, and `RIS` keeps it**: a read
/// that fails is the one error, where a stream with no reader answers it
/// before reading.
#[test]
fn a_lent_reader_serves_the_file_mediums_and_ris_keeps_it() {
    let mut k = KittyStream::NEW;
    assert_eq!(k.mediums(), KittyMediums::Direct, "Direct by default");
    k.set_files(Box::leak(Box::new(Nothing)));
    assert_eq!(k.mediums(), KittyMediums::Local);
    let (mut px, mut slots, mut places) = (
        [Pixel::CLEAR; 0],
        [ImageSlot::EMPTY; 4],
        [PlacementSlot::EMPTY; 4],
    );
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    command(&mut k, &mut store, b"a=t,i=1,t=f,f=100;L3A=");
    assert_eq!(
        k.replies(),
        b"\x1b_Gi=1;EBADF:Failed to read image file\x1b\\"
    );
    k.reset(&mut store, &mut []);
    assert_eq!(k.mediums(), KittyMediums::Local, "RIS keeps the reader");
}

/// **How many bytes a file medium reads**, decided before reading: `S`
/// when given and the object holds it, the rest of the object otherwise,
/// and past the limit the limit or a refusal.
#[test]
fn the_read_length_follows_o_s_and_the_limit() {
    let req = |offset, size, limit, past| KittyFileRequest {
        medium: KittyMedium::File,
        path: b"/p",
        offset,
        size,
        limit,
        past,
    };
    let stop = KittyPastLimit::Stop;
    let refuse = KittyPastLimit::Refuse;
    assert_eq!(req(10, 0, 100, refuse).available(50), 40);
    assert_eq!(req(60, 0, 100, refuse).available(50), 0, "O past the end");
    assert_eq!(req(0, 0, 100, refuse).read_len(40), Ok(40), "the rest");
    assert_eq!(req(0, 30, 100, refuse).read_len(40), Ok(30), "S");
    assert_eq!(
        req(0, 41, 100, refuse).read_len(40),
        Err(KittyFileFailed),
        "smaller than the client claimed"
    );
    assert_eq!(req(0, 0, 20, stop).read_len(40), Ok(20), "to the limit");
    assert_eq!(req(0, 0, 20, refuse).read_len(40), Err(KittyFileFailed));
    assert_eq!(req(0, 0, 40, refuse).read_len(40), Ok(40), "at the limit");
}
