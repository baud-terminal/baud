//! **A GIF's frames as baud's own decoder composes them**
//! (`baud_domain::gif`), for the checks `tools/baud-pictures logo` makes on
//! the README's logo. `tools/` is a workspace of its own and links no crate
//! of this one, so it runs this as it runs `flash_census`.
//!
//! usage: `gif_frames <gif> <out>`
//!
//! `<out>` takes the frames, each pixel three bytes, red, green and blue,
//! row after row, the alpha dropped. Prints one JSON object: the file's
//! `width` and `height`, and `delays`, each frame's delay in hundredths. A
//! GIF the decoder refuses is named on stderr, and the exit is 1.

use baud_domain::gif::Gif;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().collect();
    let [_, gif, out] = a.as_slice() else {
        panic!("usage: gif_frames <gif> <out>");
    };
    let data = std::fs::read(gif).expect("the GIF");
    let mut out = std::io::BufWriter::new(std::fs::File::create(out).expect("the frames' file"));
    let mut delays = Vec::new();
    let size = Gif::parse(&data).and_then(|g| {
        let mut scratch = vec![0u8; g.scratch_len()];
        g.decode(&mut scratch, |f| {
            for p in f.rgba.chunks_exact(4) {
                out.write_all(&p[..3]).expect("a frame");
            }
            delays.push((f.gap_ms / 10).to_string());
            true
        })?;
        Ok(g.size())
    });
    let size = match size {
        Ok(size) => size,
        Err(e) => {
            eprintln!("the GIF: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    out.flush().expect("the frames");
    println!(
        "{{\"width\": {}, \"height\": {}, \"delays\": [{}]}}",
        size.width,
        size.height,
        delays.join(", ")
    );
    ExitCode::SUCCESS
}
