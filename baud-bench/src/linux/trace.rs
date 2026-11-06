//! The app's pointer traffic from a compositor's `WAYLAND_DEBUG=server` log
//! (`scripts/linux-pointer.sh` drives it): the `wl_pointer` events it was sent
//! and the pointer requests it sent, one a line, without timestamps, serials
//! or object numbers, so two runs compare line for line.
//!
//! ```text
//! linux-pointer-trace < sway.log > trace.txt
//! ```
//!
//! libwayland prints a message as `[ms] -> iface@id.name(args)` for an event
//! and without the arrow for a request (`#` in place of `@` from 1.22). The
//! app is the one client that asks for a `wl_pointer` here (the harness runs
//! sway with no bar), so its seat is the one whose `get_pointer` comes first,
//! and only that seat's capabilities and the pointers it made are kept.
//! `set_cursor` and `set_shape` carry the serial of an enter, which they must
//! match -- "The serial parameter must match the latest wl_pointer.enter
//! serial number sent to the client. Otherwise the request will be ignored"
//! (wp-cursor-shape-v1.xml; wayland.xml says the same of `set_cursor`) -- and
//! each is marked when it does not.
//!
//! What the app's surfaces are drawn at is kept too, as lines that begin
//! "surface": the window's surface (the one given an `xdg_surface`) and the
//! pointer's (the one `set_cursor` names), each buffer scale the app sets on
//! them, the size of each buffer it attaches to the pointer's and of the
//! window's when it changes, the destination it sets on the window's
//! viewport, and the `preferred_buffer_scale` and the fractional
//! `preferred_scale` the compositor sends the window. The app is the one
//! client here that makes surfaces: grim, wl-paste and the virtual devices
//! make none.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use super::Done;
use super::text::{Message, message_ms, numbered};
use crate::key_latency::py;

/// `wl_pointer.button`'s codes.
const BUTTONS: [(&str, &str); 8] = [
    ("272", "left"),
    ("273", "right"),
    ("274", "middle"),
    ("275", "side"),
    ("276", "extra"),
    ("277", "forward"),
    ("278", "back"),
    ("279", "task"),
];
/// `wp_cursor_shape_device_v1.shape`, version 1's entries, from the pinned
/// XML.
const SHAPES: [&str; 34] = [
    "default",
    "context_menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical_text",
    "alias",
    "copy",
    "move",
    "no_drop",
    "not_allowed",
    "grab",
    "grabbing",
    "e_resize",
    "n_resize",
    "ne_resize",
    "nw_resize",
    "s_resize",
    "se_resize",
    "sw_resize",
    "w_resize",
    "ew_resize",
    "ns_resize",
    "nesw_resize",
    "nwse_resize",
    "col_resize",
    "row_resize",
    "all_scroll",
    "zoom_in",
    "zoom_out",
];
/// The three images the app draws without cursor-shape-v1, by their hotspot
/// (baud-present-wayland's `cursor.rs`).
const DRAWN: [(&str, &str, &str); 3] = [
    ("12", "12", "the I-beam"),
    ("4", "3", "the arrow"),
    ("9", "2", "the hand"),
];

/// A wheel axis's name.
fn axis_name(a: &str) -> &str {
    match a {
        "0" => "vertical",
        "1" => "horizontal",
        other => other,
    }
}

/// `s.rstrip("0").rstrip(".")` where `s` has a point.
fn num(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// What the walk through the log has seen.
#[derive(Debug, Default)]
struct Trace {
    seat: Option<String>,
    pointers: HashSet<String>,
    serial: Option<String>,
    window: Option<String>,
    cursor: Option<String>,
    drawn: Option<String>,
    viewport: Option<String>,
    fraction: Option<String>,
    buffers: HashMap<String, String>,
    // Lines for a surface not yet named by set_cursor: the app attaches and
    // scales the pointer's surface before it sets it.
    pending: HashMap<String, Vec<String>>,
    out: String,
}

/// `a[n]`, or the failure an index past the end is.
fn nth<'a>(a: &[&'a str], n: usize) -> Result<&'a str, String> {
    a.get(n)
        .copied()
        .ok_or_else(|| format!("no argument {n} among {a:?}"))
}

/// The number in `args` after `iface`, or the failure its absence is.
fn must<'a>(args: &'a str, iface: &str) -> Result<&'a str, String> {
    numbered(args, iface).ok_or_else(|| format!("no {iface} in {args}"))
}

impl Trace {
    fn say(&mut self, line: &str) {
        self.out += line;
        self.out.push('\n');
    }

    fn surface(&mut self, sid: &str, what: String) {
        if self.window.as_deref() == Some(sid) {
            self.say(&format!("surface window {what}"));
        } else if self.cursor.as_deref() == Some(sid) {
            self.say(&format!("surface pointer {what}"));
        } else {
            self.pending.entry(sid.to_string()).or_default().push(what);
        }
    }

    /// The window's and the pointer's surfaces: what they are drawn at.
    fn surfaces(&mut self, m: &Message<'_>, a: &[&str]) -> Result<(), String> {
        let is_window = self.window.as_deref() == Some(m.id);
        match (m.iface, m.name, m.event) {
            ("xdg_wm_base", "get_xdg_surface", false) => {
                self.window = Some(must(m.args, "wl_surface")?.to_string());
            }
            ("wl_shm_pool", "create_buffer", false) => {
                let bid = must(m.args, "wl_buffer")?.to_string();
                let size = format!("{}x{}", nth(a, 2)?, nth(a, 3)?);
                self.buffers.insert(bid, size);
            }
            ("wl_surface", "set_buffer_scale", false) => {
                let what = format!("buffer_scale {}", nth(a, 0)?);
                self.surface(m.id, what);
            }
            ("wl_surface", "attach", false) => {
                let size = numbered(m.args, "wl_buffer").map(|bid| {
                    self.buffers
                        .get(bid)
                        .cloned()
                        .unwrap_or_else(|| "?".to_string())
                });
                // The window attaches a buffer a frame: its size when it
                // changes.
                let size = size.filter(|s| !is_window || self.drawn.as_deref() != Some(s.as_str()));
                if let Some(size) = size {
                    self.surface(m.id, format!("attach {size}"));
                    if is_window {
                        self.drawn = Some(size);
                    }
                }
            }
            ("wl_surface", "preferred_buffer_scale", true) if is_window => {
                let line = format!("surface window preferred_buffer_scale {}", nth(a, 0)?);
                self.say(&line);
            }
            ("wp_viewporter", "get_viewport", false) => {
                let named = must(m.args, "wl_surface")?;
                if self.window.as_deref() == Some(named) {
                    self.viewport = Some(must(m.args, "wp_viewport")?.to_string());
                }
            }
            ("wp_viewport", "set_destination", false) if self.viewport.as_deref() == Some(m.id) => {
                let line = format!("surface window destination {}x{}", nth(a, 0)?, nth(a, 1)?);
                self.say(&line);
            }
            ("wp_fractional_scale_manager_v1", "get_fractional_scale", false) => {
                let named = must(m.args, "wl_surface")?;
                if self.window.as_deref() == Some(named) {
                    self.fraction = Some(must(m.args, "wp_fractional_scale_v1")?.to_string());
                }
            }
            ("wp_fractional_scale_v1", "preferred_scale", true)
                if self.fraction.as_deref() == Some(m.id) =>
            {
                let line = format!("surface window fractional_scale {}", nth(a, 0)?);
                self.say(&line);
            }
            _ => {}
        }
        Ok(())
    }

    /// `(NOT the last enter's serial)` when `serial` is not the enter's.
    fn mark(&self, serial: &str) -> &'static str {
        if self.serial.as_deref() == Some(serial) {
            ""
        } else {
            "  (NOT the last enter's serial)"
        }
    }

    /// A `wl_pointer` event.
    fn pointer_event(&mut self, name: &str, a: &[&str]) -> Result<(), String> {
        let line = match name {
            "enter" => {
                self.serial = Some(nth(a, 0)?.to_string());
                format!("enter {} {}", num(nth(a, 2)?), num(nth(a, 3)?))
            }
            "leave" => "leave".to_string(),
            "motion" => format!("motion {} {}", num(nth(a, 1)?), num(nth(a, 2)?)),
            "button" => {
                let code = nth(a, 2)?;
                let button = BUTTONS.iter().find(|b| b.0 == code).map_or(code, |b| b.1);
                let state = if nth(a, 3)? == "1" {
                    "pressed"
                } else {
                    "released"
                };
                format!("button {button} {state}")
            }
            "axis" => format!("axis {} {}", axis_name(nth(a, 1)?), num(nth(a, 2)?)),
            "axis_source" => format!("axis_source {}", nth(a, 0)?),
            "axis_stop" => format!("axis_stop {}", axis_name(nth(a, 1)?)),
            "axis_discrete" | "axis_value120" => {
                format!("{name} {} {}", axis_name(nth(a, 0)?), nth(a, 1)?)
            }
            _ => return Ok(()),
        };
        self.say(&line);
        Ok(())
    }

    /// `wl_pointer.set_cursor`.
    fn set_cursor(&mut self, args: &str, a: &[&str]) -> Result<(), String> {
        let mark = self.mark(nth(a, 0)?);
        let named = numbered(args, "wl_surface").filter(|n| self.cursor.as_deref() != Some(*n));
        if let Some(named) = named {
            self.cursor = Some(named.to_string());
            for what in self.pending.remove(named).unwrap_or_default() {
                self.say(&format!("surface pointer {what}"));
            }
        }
        if nth(a, 1)? == "nil" {
            self.say(&format!("set_cursor hidden{mark}"));
        } else {
            let (x, y) = (nth(a, 2)?, nth(a, 3)?);
            let image = DRAWN
                .iter()
                .find(|d| d.0 == x && d.1 == y)
                .map_or_else(|| format!("hotspot {x} {y}"), |d| d.2.to_string());
            self.say(&format!("set_cursor {image}{mark}"));
        }
        Ok(())
    }

    /// The pointers, the seat that made them and the shapes asked of them.
    fn pointers(&mut self, m: &Message<'_>, a: &[&str]) -> Result<(), String> {
        if m.iface == "wl_seat"
            && m.name == "get_pointer"
            && (self.seat.is_none() || self.seat.as_deref() == Some(m.id))
        {
            self.seat = Some(m.id.to_string());
            self.pointers
                .insert(must(m.args, "wl_pointer")?.to_string());
        }
        if m.iface == "wl_pointer" && !self.pointers.contains(m.id) {
            return Ok(());
        }
        let of_seat = self.seat.as_deref() == Some(m.id);
        match m.iface {
            "wl_seat" if m.name == "capabilities" && of_seat => {
                let caps = nth(a, 0)?;
                let caps = py::int(caps).ok_or_else(|| format!("not a number: {caps}"))?;
                let mut have = Vec::new();
                if caps & 1 != 0 {
                    have.push("pointer");
                }
                if caps & 2 != 0 {
                    have.push("keyboard");
                }
                let named = if have.is_empty() {
                    "none".to_string()
                } else {
                    have.join(" ")
                };
                self.say(&format!("capabilities {named}"));
            }
            "wl_seat" if m.name == "get_pointer" && of_seat => self.say("get_pointer"),
            "wl_pointer" if m.event => self.pointer_event(m.name, a)?,
            "wl_pointer" if m.name == "set_cursor" => self.set_cursor(m.args, a)?,
            "wl_pointer" if m.name == "release" => self.say("release"),
            "wp_cursor_shape_manager_v1" if m.name == "get_pointer" => {
                self.say("cursor-shape get_pointer");
            }
            "wp_cursor_shape_device_v1" if m.name == "set_shape" => {
                let mark = self.mark(nth(a, 0)?);
                let shape = nth(a, 1)?;
                let v = py::int(shape).ok_or_else(|| format!("not a number: {shape}"))?;
                let name = usize::try_from(v - 1)
                    .ok()
                    .and_then(|i| SHAPES.get(i))
                    .map_or_else(|| v.to_string(), |s| (*s).to_string());
                self.say(&format!("set_shape {name}{mark}"));
            }
            "wp_cursor_shape_device_v1" if m.name == "destroy" => self.say("cursor-shape destroy"),
            _ => {}
        }
        Ok(())
    }
}

/// The whole tool, over any reader. A log a client's string made not UTF-8
/// reads with a replacement character; no line that is kept holds a string.
pub fn run(mut input: impl Read) -> Done {
    let mut bytes = Vec::new();
    if let Err(e) = input.read_to_end(&mut bytes) {
        return Done::failed(String::new(), format!("stdin: {e}"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut t = Trace::default();
    // Stdin's lines end at `\n` alone, `\r` being space to `strip`.
    for raw in text.split('\n') {
        let Some(m) = message_ms(py::strip(raw)) else {
            continue;
        };
        let a: Vec<&str> = if m.args.is_empty() {
            Vec::new()
        } else {
            m.args.split(',').map(py::strip).collect()
        };
        let step = t.surfaces(&m, &a).and_then(|()| t.pointers(&m, &a));
        if let Err(why) = step {
            return Done::failed(t.out, why);
        }
    }
    Done::with(t.out, 0)
}
