//! Replay a real shell's resize traffic through the core and count what
//! ends up on the screen.
//!
//! The owner reports that every zoom leaves another copy of their prompt
//! behind. Captured from their own `zsh`, one resize looks like
//! `\r` `CUU n` `ED 0` and then the prompt redrawn -- so the shell relies
//! on `CUU n` landing on the first line of the prompt it last drew. If it
//! does not, the erase misses the old prompt and the redraw adds a second.
//!
//! This feeds the captured bytes at the captured sizes, resizing between
//! them exactly as the app does, and counts the copies. No window, no GPU,
//! no shell: the stream is a recording.
//!
//! Usage: `winch <captured.json>`.
fn main() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, Row, WinSize};

    let path = baud_bench::args()
        .next()
        .expect("usage: winch <captured.json>");
    let raw = std::fs::read_to_string(&path).expect("capture");
    // The capture is a flat array of {cols, rows, bytes:[..]} -- read with
    // a hand parser rather than a dependency, since this crate has none.
    let mut steps: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    for chunk in raw.split("{\"cols\":").skip(1) {
        let num = |key: &str| -> u16 {
            chunk
                .split(key)
                .nth(1)
                .and_then(|s| s.trim_start_matches(['"', ':']).split(',').next())
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let cols: u16 = chunk
            .split(',')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let rows = num("\"rows\"");
        let bytes: Vec<u8> = chunk
            .split("\"bytes\": [")
            .nth(1)
            .or_else(|| chunk.split("\"bytes\":[").nth(1))
            .map(|s| {
                s.split(']')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|v| v.trim().parse::<u8>().ok())
                    .collect()
            })
            .unwrap_or_default();
        steps.push((cols, rows, bytes));
    }
    let geom = baud_bench::host_geometry();
    let max = WinSize {
        ws_col: 400,
        ws_row: 150,
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::with_geometry(&max, &geom)));
    let first = steps.first().expect("a first step");
    let ws = WinSize {
        ws_col: first.0,
        ws_row: first.1,
    };
    let mut t = Term::new(owned.storage(), &ws);
    let mut last_count = 0usize;
    for (cols, rows, bytes) in &steps {
        let _ = t.resize(&WinSize {
            ws_col: *cols,
            ws_row: *rows,
        });
        for chunk in bytes.chunks(4096) {
            t.write_at(BytePos::ZERO, chunk);
            t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        }
        let (c, r) = t.geometry();
        {
            let mut on = 0;
            for y in 0..r.index() {
                let line: String = t
                    .row(Row::from_index(y))
                    .iter()
                    .map(|cl| match cl.content.as_codepoint() {
                        Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                        None => ' ',
                    })
                    .collect();
                if line.trim_end().starts_with("zsh ") {
                    on += 1;
                }
            }
            println!("  -> prompt copies on screen: {on}");
            // When the count rises, print the screen: what the bytes did
            // is then visible rather than inferred.
            if on > last_count {
                println!("  === screen after the step that added one ===");
                for y in 0..r.index() {
                    let line: String = t
                        .row(Row::from_index(y))
                        .iter()
                        .take(c.index())
                        .map(|cl| match cl.content.as_codepoint() {
                            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                            None => ' ',
                        })
                        .collect();
                    let line = line.trim_end();
                    if !line.is_empty() {
                        println!("    {y:3}| {}", &line[..line.len().min(60)]);
                    }
                }
                println!(
                    "  === bytes: {:?}",
                    String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
                        .replace('\u{1b}', "<E>")
                );
            }
            last_count = on;
        }
        println!(
            "after {}x{}: grid {}x{}, {} bytes, cursor {:?}, retained {}",
            cols,
            rows,
            c.index(),
            r.index(),
            bytes.len(),
            t.cursor(),
            t.scrollback().len()
        );
    }
    // How many rows of the final screen and its history start the prompt.
    let text = |cells: &[baud_core::PackedCell]| -> String {
        cells
            .iter()
            .map(|c| match c.content.as_codepoint() {
                Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                None => ' ',
            })
            .collect()
    };
    let (_, rows) = t.geometry();
    let mut on_screen = 0;
    // Where each row's content actually stops, against the grid's width:
    // the question a truncated right-hand side is.
    {
        let (c, r) = t.geometry();
        println!(
            "--- rightmost non-blank column, grid is {} wide ---",
            c.index()
        );
        for y in 0..r.index() {
            let line = text(t.row(Row::from_index(y)));
            let last = line.trim_end().chars().count();
            if last > 0 {
                println!("  row {y:3}: content to column {last}");
            }
        }
    }
    println!("--- final screen ---");
    for y in 0..rows.index() {
        let line = text(t.row(Row::from_index(y)));
        let line = line.trim_end();
        if line.starts_with("zsh ") {
            on_screen += 1;
        }
        if !line.is_empty() {
            println!("  {y:3}: {}", &line[..line.len().min(72)]);
        }
    }
    let mut retained = 0;
    let sb = t.scrollback();
    for n in 0..sb.len() {
        if let Some((c, _)) = sb.line(baud_domain::Back::of(n)) {
            if text(c).trim_end().starts_with("zsh ") {
                retained += 1;
            }
        }
    }
    println!("prompt copies: {on_screen} on screen, {retained} in history");
}
