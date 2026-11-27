//! **A folder as a window's title spells it, and text as a title shows
//! it**: pure, so the title (`title.rs`) and the demo's search of what a
//! session showed (`undrawn.rs`, through `#[path]`) read it alike.

/// Text as a title shows it: controls as spaces, the ends trimmed.
pub fn shown(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().to_string()
}

/// **A folder as the title spells it**: the path with the home directory
/// as `~`, and past three folders an ellipsis and the last three --
/// `~/src/baud`, `…/baud/baud-core/src` -- as kitty's and ghostty's own
/// shell integrations title a prompt, with zsh's `%(4~|…/%3~|%~)` (kitty's
/// zsh integration 271-281, ghostty's 243-248). Counted as zsh counts,
/// measured: `~` is a folder and the root is not, so `/usr/share/man` is
/// whole and `/usr/share/man/man1` is `…/share/man/man1`. This is chosen
/// over the whole path and over the last folder alone. Shown as a title
/// shows text.
pub fn folder_text(path: &[u8], home: Option<&[u8]>) -> String {
    let home = home.filter(|h| h.len() > 1 && !h.ends_with(b"/"));
    let spelled: Vec<u8> = match home {
        Some(h) if path == h => b"~".to_vec(),
        Some(h) if path.starts_with(h) && path.get(h.len()) == Some(&b'/') => {
            [&b"~"[..], &path[h.len()..]].concat()
        }
        _ => path.to_vec(),
    };
    let folders: Vec<&[u8]> = spelled
        .split(|&b| b == b'/')
        .filter(|f| !f.is_empty())
        .collect();
    let shortened = match folders.len() {
        0..=3 => spelled,
        n => ["\u{2026}/".as_bytes(), &folders[n - 3..].join(&b'/')].concat(),
    };
    shown(&String::from_utf8_lossy(&shortened))
}
