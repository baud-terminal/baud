// Legal uses compile; the compile_fail cases live in doc comments and in the
// notes below. Each rejected case is a real bug the model prevents.
use baud_domain::*;

#[test]
fn legal_uses_compile() {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let (cols, rows) = ws.geometry();

    // typed grid: indexed by axis, never by integer
    let mut buf = vec![Cell2::default(); 80 * 24];
    let mut g = Grid::new(&mut buf, cols, rows);
    let r = OneBased::from_param(None).to_row(rows);
    let c = OneBased::from_param(None).to_col(cols);
    g[(r, c)] = Cell2 { v: 7 };
    assert_eq!(g[(r, c)].v, 7);

    // cx += k, typed and total
    let mut cx = Col::ZERO;
    let room = cols.room_at(cx);
    cx += room;
    assert!(cols.room_at(cx).is_empty());

    // CSI parsing speaks types end to end
    let ps = [
        Some(Param::ZERO.push(dig(5))),
        Some(Param::ZERO.push(dig(1)).push(dig(0))),
    ];
    match Csi::parse(
        Marker::Standard,
        // No intermediate: `CSI 5;10 H` is CUP. With one, the same final byte
        // would be a different operation entirely.
        None,
        Params::new(&ps),
        FinalByte::from_stream(StreamByte::from_stream(b'H')).unwrap(),
    ) {
        Csi::CursorPosition { .. } => {}
        _ => panic!("expected CursorPosition"),
    }

    // line lifecycle
    let l = Line::new(RunIdx::FIRST).complete(DisplayWidth::ZERO);
    let _ = l.width();
}

fn dig(n: u8) -> Digit {
    StreamByte::from_stream(b'0' + n).as_ascii_digit().unwrap()
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Cell2 {
    pub v: u16,
}
