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
