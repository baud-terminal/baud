//! The terminal's configuration: which faces, what size, how translucent.
//!
//! A small, dependency-free reader for a subset of TOML -- `key = value`,
//! string, integer and decimal scalars, a bracketed list of strings that may
//! span lines, and `[section]` headers. baud owns its stack rather than
//! pulling `serde` and a TOML crate for a handful of keys -- a font list,
//! `font_size`, `padding`, `colorspace`, `vs16_width`, `emoji`, `theme`, `bell`,
//! `[text] ambiguous_width`, `[screen] checksum`, `logging`, `printing` and the four `print_`
//! choices, `[pointer]
//! hide_while_typing`, `[font] changes`, and the eight under
//! `[glass]` -- and a key it does not understand is ignored, and said,
//! rather than fatal, so a newer config does not break an older binary.
//!
//! **Sections are a namespace, and they did not used to be.** The reader
//! flattened them, so `[glass] style` and a top-level `style` were one key;
//! `[glass]`'s eight names are generic enough (`hue`, `shadow`, `style`)
//! that flattening them would have put them in the same space as every
//! future top-level key. A config that put a top-level key under some
//! header stops being read -- neither committed config does.
//!
//! The file is looked up at `$BAUD_CONFIG`, then `$XDG_CONFIG_HOME/baud/
//! config.toml`, then `~/.config/baud/config.toml`, and in the working
//! directory only as far as a relative `$BAUD_CONFIG` names a file under
//! it; a relative font or theme path in it names a file beside it
//! ([`config_paths`]). When none is found or a field is absent,
//! the defaults are the stack that starts with baud's embedded font --
//! JetBrains Mono, with baud's Hebrew in its Regular and Bold -- and its
//! Nerd Fonts icon face, with the embedded
//! Bold, Italic and Bold Italic as the styled faces -- so the app runs with
//! no config at all. A configured `fonts` replaces that stack and keeps the
//! embedded Regular, the icon face and then STIX Two Math behind it, for the
//! Hebrew, icons and symbols the named faces lack (the owner's order:
//! configured, then baud's embedded font, then the system, which is not
//! built for a configured stack yet (note AC decision 9); and the owner's
//! call of 2026-10-03, as every terminal that bundles faces keeps them behind
//! a configured font), unless `font_fallback = false`. A configured stack's
//! styled faces are the Bold, Italic and Bold Italic of the family of the
//! first of its fonts that is found, picked by CSS Fonts' style search ([`Config::resolve`]), and a
//! style the family has no nearer face for is drawn from its regular face,
//! never synthesised.
//!
//! **A font is named by its family**, as ghostty's `font-family`, kitty's
//! `font_family`, alacritty's `font.normal.family` and WezTerm's
//! `wezterm.font` take one, and the default stack's system faces are names
//! too: [`Config::resolve`] finds each among the installed faces once at
//! launch (`discover.rs`), and a file path stays accepted, as theirs do.

use baud_domain::{CursorBlink, CursorShape};

/// A face is a file and, for a collection (`.ttc`), which face in it: the
/// path may carry `@N`, so `Menlo.ttc@0`. Or it is the font compiled into
/// the binary, whose `path` is only a name for the log. Or, until the
/// config is resolved at launch, a family name, which `path` holds.
#[derive(Clone)]
pub struct FaceSpec {
    pub path: String,
    pub index: baud_font::FaceIndex,
    pub from: FaceFrom,
    /// Part of the fallback chain, or the face one style is drawn from.
    pub role: baud_render::FaceRole,
    /// The axis values a variable face is drawn at; none for a static one.
    pub variations: Vec<baud_font::var::Variation>,
}

/// Where a face's bytes come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceFrom {
    /// A file on disk, mapped read-only at launch.
    File,
    /// Bytes compiled into the binary.
    Embedded(&'static [u8]),
    /// A family name, which [`Config::resolve`] turns into the file of the
    /// installed face it picks, or drops.
    Family,
}

/// Whether [`Config::resolve`] has found a face a config named yet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    None,
    Some,
}

/// Whether the stack is the built-in one or one a config's `fonts` named:
/// a name a config gave that matches nothing is said, one of the defaults
/// that is not installed is left out quietly, as a default path that did
/// not exist was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackFrom {
    Default,
    Configured,
}

/// `[screen] badge`: whether a program's badge is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeShown {
    On,
    Off,
}

/// A `[screen]` print choice's value: `"on"` is true, `"off"` false, and
/// anything else false, said.
fn on_off(key: &str, value: &str, said: &mut Vec<String>) -> bool {
    match value.trim().trim_matches('"') {
        "on" => true,
        "off" => false,
        other => {
            said.push(format!(
                "config: [screen] {key} {other:?} is not on or off; using off"
            ));
            false
        }
    }
}

/// `[screen] notifications`' two values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notifications {
    On,
    Off,
}

/// `[screen] printing`'s two values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Printing {
    On,
    Off,
}

/// Whether the embedded faces follow a configured stack -- the embedded
/// Regular, then the icon face, then STIX Two Math -- so a glyph none of the
/// named faces carries, Hebrew or a Nerd Fonts icon, draws from them instead
/// of as `.notdef`. kitty, wezterm and ghostty all put their bundled symbols
/// behind the user's fonts the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFallback {
    Embedded,
    Off,
}

/// `config_reload`: whether a change to the config file, or to a theme or
/// font file it names, is applied to the running window. **On by
/// default**, as alacritty's `live_config_reload` and WezTerm's
/// `automatically_reload_config` are (Jev's lean after a re-ask, ON, 0.88
/// and 0.90); the name is Jev's lean, `config_reload`, 0.18 and 0.37.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigReload {
    On,
    Off,
}

#[derive(Clone)]
pub struct Config {
    /// Where the file was found, so the startup log can say which config
    /// is in force -- a `baud` launched outside the checkout once fell back
    /// to the Menlo stack in silence, and looked like a font that had
    /// stopped being respected.
    pub source: Option<std::path::PathBuf>,
    /// The fallback stack, first face first.
    pub faces: Vec<FaceSpec>,
    /// Whether `faces` is the built-in stack or a config's.
    pub stack: StackFrom,
    /// Whether the embedded Regular, the icon face and STIX Two Math follow
    /// a `fonts` stack the config names, already folded into `faces` by the
    /// time the config is read.
    pub font_fallback: FontFallback,
    /// Points; pixels are this times the backing scale.
    pub font_size: f32,
    /// The gutter, in points, left of and right of and below the grid, so
    /// the glass shows through around the terminal rather than the cells
    /// meeting the window's edge. The titlebar strip is the top gutter and
    /// is not this.
    pub padding: f32,
    /// What the surface's bytes mean: sRGB, which is what every colour a
    /// terminal receives is specified as, or Display P3 for a reader who
    /// wants the theme's numbers read as the wider gamut on purpose. The
    /// compositor converts to each display from there.
    pub colorspace: Space,
    /// Which palette a rendition resolves against, and whether the system
    /// decides. `Auto` follows the effective appearance -- the macOS
    /// driver re-reads it on every style change; the Linux driver has no
    /// signal wired yet and resolves it to dark.
    pub theme: ThemeChoice,
    /// The contrast floor, from `[palette] floor`: a trigger, or off.
    pub floor: baud_render::Floor,
    /// `[palette] light` and `dark`: the theme files for the two halves, as
    /// written. Stored and not read here -- loading them is I/O, with its
    /// own complaints, in `palette.rs`.
    pub palette_light: Option<String>,
    pub palette_dark: Option<String>,
    /// `[palette] selection` and `selection_text`: the selection's colours
    /// over both halves' theme files ([`Selection`]).
    pub selection: Selection,
    /// `[palette] cursor` and `cursor_text`: the cursor's colour and the
    /// text under a block, over both halves' theme files
    /// ([`CursorColours`]).
    pub cursor: CursorColours,
    /// `[cursor] trail`: which trail a cursor move leaves -- see
    /// [`CursorTrail`] -- or the default, which is not the same thing:
    /// [`TrailPick`]. **On by default**, the owner's call on 2026-09-25,
    /// where sfte ships its own `SFTE_CURSOR_TRAIL` off: an idle terminal
    /// pays nothing for it, because only a cursor that moved spawns one.
    /// The default is the plasma, the owner's call of 2026-09-29.
    /// `false` turns it off, and the default goes off under Reduce Motion
    /// where a named trail does not.
    pub cursor_trail: TrailPick,
    /// `[cursor] shape` and `[cursor] blink`: the cursor the terminal powers
    /// on with, and the one an application's reset (`DECSCUSR 0` or `7`,
    /// `DECSTR`, `RIS`) returns to. A blinking block by default, the owner's
    /// call on 2026-09-25 and `DECSCUSR`'s own default. Two keys and two
    /// fields, because the keys may come in either order.
    pub cursor_shape: CursorShape,
    /// `[cursor] blink`: `"auto"` by default, which blinks as the system
    /// blinks its caret -- at its rate, and steady under its non-blinking
    /// switch -- and `true` or `false`; see [`BlinkChoice`].
    pub cursor_blink: BlinkChoice,
    /// `[cursor] stop_blinking_after`: seconds without input after which a
    /// blinking cursor stops, shown; never by default, the owner's call.
    pub cursor_stop: StopAfter,
    /// `[cursor] blink_animation`: what a blinking cursor does in its
    /// hidden phase -- the plasma glow by default, the owner's pick of
    /// 2026-10-15; see [`BlinkAnimation`].
    pub blink_animation: BlinkAnimation,
    /// `[cursor] blink_interval_ms`: the length of each phase, shown and
    /// hidden alike, if the config sets one; see [`BlinkInterval`].
    pub blink_interval: BlinkInterval,
    /// `[cursor] guide`: whether the cursor's row is highlighted -- iTerm2's
    /// cursor guide -- `"off"` by default, as iTerm2's profile has it, or
    /// `"on"`. A program's `OSC 1337 ; HighlightCursorLine` overrides it
    /// until a reset (`Term::cursor_guide`).
    pub cursor_guide: baud_domain::CursorGuide,
    /// `[screen] transition`: whether a switch to or from the alternate
    /// screen fades the new one in from the background. Off by default.
    pub screen_transition: ScreenTransition,
    /// `[screen] exit_status`: whether each command's exit status, which a
    /// shell's prompt marks report (`OSC 133 ; D`), is shown as a mark in a
    /// strip one cell wide beside the text, the grid giving it a column;
    /// `"off"` by default (Jev's lean, `baud_render::status`), `"on"` shows
    /// it.
    pub exit_status: baud_render::StatusMarks,
    /// `[screen] badge`: whether a program's badge -- iTerm2's large label
    /// in the top right behind the text (`OSC 1337 ; SetBadgeFormat`) -- is
    /// drawn, `"on"` by default, as iTerm2 draws every badge a program
    /// sets, or `"off"`.
    pub badge: crate::config::BadgeShown,
    /// `[screen] checksum`: whether a program's request for a checksum of
    /// the screen (DECRQCRA) is answered, `"on"` by default (the
    /// owner's call of 2026-10-19, option 3), or `"off"`, which leaves every
    /// request unanswered. A checksum of one cell at a time reads the screen
    /// back, other programs' output included.
    pub checksum: baud_domain::ChecksumReport,
    /// `[screen] logging`: whether a program may log what it sends to a
    /// file it names (xterm's `?46` and `OSC 46`), `"on"` by default (the
    /// owner's call of 2026-10-20), or `"off"`, which refuses both. A
    /// program on the other end of the line, a remote one included, can
    /// otherwise append its output to any file the user owns that no one
    /// else may write.
    pub logging: baud_domain::Logging,
    /// `[screen] printing`: whether a program's print request opens the
    /// print panel (MC, autoprint and printer controller mode), `"on"` by
    /// default (Jev's lean, SETTING, 0.70/0.82 then 0.82/0.88), or `"off"`,
    /// under which the core is told there is no printer: DSR `?15` answers
    /// "no printer" and every print request is ignored and counted. Nothing
    /// prints without the user pressing Print in the panel either way.
    pub printing: Printing,
    /// `[screen] notifications`: whether a program can post a desktop
    /// notification (`OSC 9`, `OSC 777 ; notify`, kitty's `OSC 99`),
    /// `"on"` by default, or `"off"`, under which the core is told nothing
    /// is delivered: every notification is refused and counted, and
    /// kitty's query goes unanswered (`notify.rs`). A program's progress
    /// bar shows either way.
    pub notifications: Notifications,
    /// `[screen] print_colour`, `print_rgb`, `print_background` and
    /// `print_rotated`: how a program's printed picture (a ReGIS hard copy)
    /// is sent, the choices DEC STD 070 leaves to the user (070-7 § 7.8.2):
    /// in colour or black and white, its colours named in RGB or HLS, with
    /// its background or without, turned a quarter or not. Each `"off"` by
    /// default, the VT330/VT340's defaults.
    pub graphics_print: baud_domain::GraphicsPrint,
    /// `[window] operations`: whether a program's window operations
    /// (XTWINOPS 1-10 and DECSLPP: move, resize, minimise, raise, lower,
    /// maximise, full screen) are performed, `"on"` by default (the owner's
    /// call of 2026-10-19, option 4), or `"off"`, which refuses every one.
    /// A program on the other end of the line, a remote one included, can
    /// otherwise move or hide the window.
    pub window_ops: baud_domain::WindowOps,
    /// `[pointer] hide_while_typing`: whether a key hides the pointer
    /// unless a program tracks the mouse, `"on"` by default -- xterm's
    /// `pointerMode` default, 1 -- or `"off"`, which never hides it, xterm's
    /// 0 (the owner's on/off settings; Jev, ON_OFF, 0.96/0.91). What the
    /// terminal powers on in and `RIS` returns to; a program may still
    /// choose any mode with XTSMPOINTER.
    pub pointer_mode: baud_domain::PointerMode,
    /// `[font] changes`: whether a program's `OSC 50` may change the font
    /// -- a face baud has loaded, by family, full or PostScript name, or a
    /// step of the size -- `"on"` by
    /// default, or `"off"`, which refuses every change; a program can ask
    /// which font is in use either way (the owner's call of 2026-10-20,
    /// option D).
    pub font_changes: baud_domain::FontChanges,
    /// `[scroll] mode`: whether output scrolls the screen up in one jump
    /// (`normal`) or slides it smoothly (`smooth`). `Smooth` by default,
    /// matching sfte -- the slide is bounded to one row so a burst never
    /// accumulates lag, and an idle screen pays nothing, so the latency is
    /// bounded rather than a reason to keep it off. `normal` opts out.
    pub scroll: ScrollMode,
    /// `[cursor] trail_scroll`: whether a trail in flight rides a smooth
    /// scroll -- `ride` by default -- or stays.
    pub trail_scroll: TrailScroll,
    /// The window's material and how it is dimmed, from `[glass]`.
    pub glass: Glass,
    /// How much caller memory the image store is given, from `[images]`.
    ///
    /// One knob, `pixels_mb`, bounding the *decoded* ARGB images (a
    /// 1024x1024 image is 4 MiB whatever its Sixel encodes to): a Sixel is
    /// decoded as its body arrives and no body is held, so its bytes are no
    /// limit (`host::IMAGE_BUDGET`). An image past it is refused and counted
    /// rather than truncated. `body_kb`, the second knob until 2026-10-14,
    /// bounded a raw body held before it decoded, and is read now only to say
    /// it no longer limits anything.
    pub images: baud_domain::ImageBudget,
    /// Whether the Primary DA advertises Sixel, from `[images] advertise_sixel`.
    /// **Off by default**: an application that sees the claim draws Sixel instead
    /// of its text preview -- chafa's grid of block elements, box-drawing
    /// strokes and coloured spaces, for yazi -- and the claim is the reader's
    /// to make rather than baud's to guess.
    pub sixel: baud_domain::SixelClaim,
    /// `vs16_width`: whether a `U+FE0F` makes a text-default emoji two cells,
    /// `"two"` by default (owner, 2026-10-01), or one for a session whose
    /// programs measure with a legacy `wcwidth`. `DECRQM ?2027` reports it.
    pub vs16_width: baud_domain::Vs16Width,
    /// `[text] ambiguous_width`: whether an East Asian Ambiguous character
    /// takes one column, `"narrow"`, the default, or two, `"wide"` (note V,
    /// V14, the owner's call of 2026-09-25). `[[source: uax11]]` § 5: narrow
    /// when the context cannot be established, and a terminal cannot see
    /// the program's width function. Private Use Area characters, the icons,
    /// stay one column under either (the owner, 2026-10-15). `DECRQM ?1021`
    /// reports it.
    pub ambiguous_width: baud_domain::AmbiguousWidth,
    /// `emoji`: how a character with no variation selector is drawn --
    /// `"font"`, the default, from a text face whatever its default
    /// presentation; `"system"`, an emoji-presentation character in colour
    /// as macOS draws it. A `U+FE0F` asks for colour and a `U+FE0E` for text
    /// under either (owner, 2026-09-30).
    pub emoji: baud_render::EmojiDefault,
    /// `bell`: what a `BEL` does in a focused window -- the system's alert
    /// sound by default, the visual bell, or nothing (the owner, 2026-10-13;
    /// [`BellStyle`]). `?1042` and `?1043` act on an unfocused window
    /// whatever this says (`bell.rs`, `ring`).
    pub bell: BellStyle,
    /// `[clipboard]`: whether a program's `OSC 52` copy reaches the
    /// pasteboard, and how large a copy the terminal holds (note V, V11).
    pub clipboard: Clipboard,
    /// `config_reload`: whether the running window follows the file.
    pub config_reload: ConfigReload,
}

/// `[clipboard] osc52`: whether a program may write the clipboard with
/// `OSC 52`. **`Write` by default**, the owner's V11 (2026-09-25), as kitty
/// and sfte do; reads stay refused whatever this says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc52 {
    Write,
    Off,
}

/// `[clipboard] copy_on_select`: whether finishing a selection copies it,
/// note V's V13. **`On` by default**, the owner's decision (2026-09-25):
/// on macOS a finished selection goes to the pasteboard; `Off` copies one
/// only while a program has set `?1041`. On Linux a finished selection
/// sets the primary either way, as Linux terminals do, and the clipboard
/// under `?1041` (Jev, PRIMARY_STAYS, 0.90/0.92, and MODE_WINS, the lean,
/// 0.42/0.36), so the key changes nothing there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyOnSelect {
    On,
    Off,
}

/// The `[clipboard]` section.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Clipboard {
    pub osc52: Osc52,
    pub copy_on_select: CopyOnSelect,
    /// `body_kb`: the largest `OSC 52` body the terminal holds, in KiB of
    /// base64 -- three octets of text for every four bytes. The core keeps
    /// two areas of the text, the copy being collected and the last one
    /// finished, so the default 256 holds a 192 KiB copy in 384 KiB.
    pub budget: baud_domain::ClipboardBudget,
}

impl Clipboard {
    /// Writes on, and a 256 KiB body: note V's design, item 8.
    pub const DEFAULT: Clipboard = Clipboard {
        osc52: Osc52::Write,
        copy_on_select: CopyOnSelect::On,
        budget: baud_domain::ClipboardBudget::of_body(256 << 10),
    };
}

/// The material, and the dimming over it.
///
/// **Every value here is one the crate's own example sweeps with a `GN_*`
/// environment variable** -- `examples/common/mod.rs` in
/// `macos-liquid-glass`, the content both `titled` and `borderless` draw,
/// and what was built as `glassterm` before the examples were renamed.
/// **The material and the two dims follow Apple's guidance; the rest is
/// the example's calibration.** The HIG puts a standard material behind
/// app backgrounds and Liquid Glass on controls
/// ([[source: apple-hig-materials]]), and a terminal's body is its
/// content; the dims
/// are Jev's decided 0.70 and 0.60, which keep default text at or above
/// 4.5:1, the HIG's minimum ([[source: apple-hig-dark-mode]]), over flat
/// white and flat black with the material darkening nothing;
/// `the_default_dims_keep_text_readable_over_white_and_black` computes
/// that, and is a floor under them, not their derivation.
/// The tints, the hue, the saturation and the shadow are the example's,
/// which its header records as "tuned against the system widget over one
/// wallpaper rather than derived from a measurement" -- a starting point a
/// reader adjusts, which is why every value here is a key; the tints, hue
/// and saturation darken the glass only, since the standard material has
/// no tint.
///
/// **Four of the eight are pairs, because the example's are.** The
/// material darkening and the body dim both change with the system's
/// light/dark setting; baud had one fixed `background_alpha` and no
/// material darkening at all, which is the whole of why its glass looked
/// nothing like the example's. Nothing here resolves the pair: that needs
/// the appearance, which is the driver's to read, and this file compiles
/// on Linux where there is no such signal.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Glass {
    /// `style`: which material. `Standard`, the default, is the HIG's
    /// "Standard materials" for "app backgrounds", where "Don't use Liquid
    /// Glass in the content layer" ([[source: apple-hig-materials]]; note
    /// AT). `Regular` is the glass the HIG gives surfaces with "a
    /// significant amount of text"; `Clear` the example's, which it keeps
    /// for components "that appear over visually rich backgrounds". One
    /// key with a third value rather than a sibling, so a config naming a
    /// glass keeps it: Jev's lean, 0.83/0.73 and after research 0.79/0.77
    /// (`decisions.jsonl` 796-799).
    pub style: Material,
    /// `dim_dark`: the body dim under a dark appearance, 0 to 1. The
    /// palette's background is painted at this.
    pub dim_dark: f32,
    /// `dim_light`: the same under a light appearance.
    pub dim_light: f32,
    /// `tint_dark`: the material darkening under a dark appearance, 0 to
    /// 1 -- applied to the material itself rather than as a layer over
    /// it, so the titlebar strip and the body share one surface.
    pub tint_dark: f32,
    /// `tint_light`: the same under a light appearance. A black strong
    /// enough for Dark would wrongly darken Light, which is the reason
    /// the pair exists.
    pub tint_light: f32,
    /// `hue`: the material darkening's hue, 0 to 1. Not black: black
    /// scales every channel equally, so it cannot shift the material's
    /// hue, only scale the colour down -- the harder it darkens, the less
    /// colour survives.
    pub hue: f32,
    /// `saturation`: the material darkening's saturation, 0 to 1.
    pub saturation: f32,
    /// `shadow`: whether the window casts one.
    pub shadow: Shadow,
    /// `app_backgrounds`: `"dim"`, the default, draws every background an
    /// application sets under the same dim as the default one -- the
    /// owner's 2026-09-20 rule, so btop's panels are glass; `"opaque"`
    /// draws them all as the application chose them and leaves only the
    /// default background glass, so btop's panels are solid too. A
    /// file manager's text-drawn photo preview no longer needs `"opaque"`:
    /// under `"dim"`, `pictures` below finds it and draws it solid. It
    /// replaced `images`, a block-elements-only rule that covered as
    /// little as 19% of a preview's cells; see `baud_render::AppBg`.
    pub app_backgrounds: baud_render::AppBg,
    /// `pictures`: `"opaque"`, the default, draws the backgrounds of a
    /// picture drawn in text -- a connected block of set backgrounds
    /// holding sixteen colours or more, which a chafa preview is and a TUI
    /// panel is not -- solid under `app_backgrounds = "dim"`, so the
    /// picture does not split into solid glyphs beside see-through holes.
    /// `"dim"` dims them like every other background. See
    /// `baud_render::PictureBg` for the measurement behind the threshold.
    pub pictures: baud_render::PictureBg,
}

/// Which material the window's body is: the see-through standard
/// material, or an `NSGlassEffectView` style.
///
/// Named for what it selects rather than for the crate's types, which
/// are macOS-only: this one has to compile on Linux, so it carries the
/// choice and the driver maps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Material {
    /// `.underWindowBackground`, behind-window blending, its state
    /// following the window's: Apple's standard material for app
    /// backgrounds ([[source: apple-hig-materials]]; note AT). The default
    /// since the owner took that recommendation, 2026-10-18.
    #[default]
    Standard,
    /// Clear Liquid Glass.
    Clear,
    /// Regular Liquid Glass.
    Regular,
}

/// `auto` casts a shadow only under a dark appearance: over a light
/// material a window shadow draws as a hard black outline, which none of
/// the system's own light glass surfaces have.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shadow {
    #[default]
    Auto,
    Always,
    Never,
}

impl Glass {
    /// The standard material, dims of 0.70 and 0.60 -- where the example's
    /// `GN_GLASS` is clear, `GN_PAPER_D` 0.22 and `GN_PAPER_L` 0.12, which
    /// put default text at 1.14:1 and 1.13:1 over white and black -- and
    /// the rest the example's own `GN_*` defaults: `GN_GT_D` 0.54,
    /// `GN_GT_L` 0.14, `GN_HUE` 0.60, `GN_HSAT` 0.80, `GN_SHADOW` auto.
    pub fn defaults() -> Glass {
        Glass {
            style: Material::Standard,
            dim_dark: 0.70,
            dim_light: 0.60,
            tint_dark: 0.54,
            tint_light: 0.14,
            hue: 0.60,
            saturation: 0.80,
            shadow: Shadow::Auto,
            app_backgrounds: baud_render::AppBg::Dimmed,
            pictures: baud_render::PictureBg::Opaque,
        }
    }
}

/// The palette, or the instruction to follow the system's appearance.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeChoice {
    /// Follow the effective appearance, and change with it.
    #[default]
    Auto,
    Dark,
    Light,
}

/// Which trail a cursor move leaves, from `[cursor] trail`: a plasma over
/// kitty's quad -- the default, the owner's call of 2026-09-29, replacing
/// the fire of 2026-09-25, and drawn as the quad on the CPU path -- a fire
/// rising off the quad and the rest of the effect family, kitty's quad,
/// whose four corners chase the cursor's, sfte's sweep, a box swept from a
/// tail chasing the cursor and fading out toward it, baud's first trail, a
/// one-cell ghost sweeping between the cells, or none. `true` is the
/// default and `false` is `off`, so a config written when the key was a
/// switch reads as it did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorTrail {
    Off,
    Ghost,
    Sweep,
    Quad,
    Fire,
    #[default]
    Plasma,
    Lightning,
    Ice,
    Ufo,
    Orb,
    Liquid,
}

impl CursorTrail {
    /// What the trail driver runs for this trail: a fire, a sweep, or a
    /// quad for the rest, which either are the quad or never ask the
    /// driver.
    pub fn style(self) -> crate::trail::Style {
        use crate::trail::Style;
        use baud_render::Shape;
        use baud_render::effect::Effect;
        match self {
            CursorTrail::Fire => Style::Effect(Effect::Fire),
            CursorTrail::Plasma => Style::Effect(Effect::Plasma),
            CursorTrail::Lightning => Style::Effect(Effect::Lightning),
            CursorTrail::Ice => Style::Effect(Effect::Ice),
            CursorTrail::Ufo => Style::Effect(Effect::Ufo),
            CursorTrail::Orb => Style::Effect(Effect::Orb),
            CursorTrail::Liquid => Style::Effect(Effect::Liquid),
            CursorTrail::Sweep => Style::Smear(Shape::Sweep),
            CursorTrail::Off | CursorTrail::Ghost | CursorTrail::Quad => Style::Smear(Shape::Quad),
        }
    }
}

/// What `[cursor] trail` said: nothing -- the default -- or a trail by name.
///
/// **The two are told apart because Reduce Motion reads them apart**, the
/// owner's call on 2026-09-25: "Reduce Motion means no trail, unless the
/// config names one." So a config with no `trail`, `trail = true` -- the word
/// for the default since the key was a switch -- and a value that names no
/// trail are all `Default`, and follow the system; any trail by name, `off`
/// included, is `Named` and is what the reader asked for whatever the system
/// says. A typo must not override an accessibility setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TrailPick {
    #[default]
    Default,
    Named(CursorTrail),
}

/// The system's motion preference: macOS's Reduce Motion, read by the
/// driver. It decides the default trail and holds `SGR 5` text shown. The
/// Linux driver reads no desktop preference yet, so it is full there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemMotion {
    Full,
    Reduced,
}

impl TrailPick {
    /// The trail drawn: a named one whatever the system says, and the
    /// default one -- the plasma -- unless the system asked for reduced
    /// motion, when none.
    pub fn effective(self, motion: SystemMotion) -> CursorTrail {
        match (self, motion) {
            (TrailPick::Named(t), _) => t,
            (TrailPick::Default, SystemMotion::Full) => CursorTrail::default(),
            (TrailPick::Default, SystemMotion::Reduced) => CursorTrail::Off,
        }
    }
}

/// `[cursor] blink`: whether the cursor blinks, or whether the system says.
///
/// **`Auto` is the default, and it is the system's caret**, the owner's call
/// of 2026-09-25: the rate macOS gives its own insertion point, and steady
/// under its non-blinking-caret switch. `On` blinks whatever the switch says,
/// and says so once in the log; `Off` powers the cursor on steady. What an
/// application sets with `DECSCUSR` is what the terminal answers either way;
/// the switch changes only what is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlinkChoice {
    Auto,
    On,
    Off,
}

impl BlinkChoice {
    /// The cursor the terminal powers on with: `Auto` and `On` blink, as
    /// the core has always powered on; `Off` is steady.
    pub fn power_on(self) -> CursorBlink {
        match self {
            BlinkChoice::Auto | BlinkChoice::On => CursorBlink::Blinking,
            BlinkChoice::Off => CursorBlink::Steady,
        }
    }
}

/// `[cursor] blink_animation`: what a blinking cursor does in its hidden
/// phase, the owner's picks of 2026-10-15. **`Plasma` by default**: the
/// plasma trail's glow on the cursor's cell, pulsing out and back across the
/// hidden phase. `Fade` fades the cursor out over the first 30% of the
/// hidden phase and in over the last 30%, eased; `Fire` burns the cursor
/// down into embers and relights it, the fire trail's look on its cell;
/// `Ufo` lifts it away on a saucer's beam and sets it back down, the ufo
/// trail's; `None` is a hard switch.
/// Each runs inside the hidden phase and scales with it, so any
/// `blink_interval_ms` works with any of them. Reduce Motion always means
/// `None` ([`BlinkAnimation::effective`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BlinkAnimation {
    #[default]
    Plasma,
    Fade,
    Fire,
    Ufo,
    None,
}

impl BlinkAnimation {
    /// The animation drawn: the configured one, and none under Reduce
    /// Motion whatever the config says -- the owner's "Reduce Motion always
    /// means none".
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn effective(self, motion: SystemMotion) -> BlinkAnimation {
        match motion {
            SystemMotion::Full => self,
            SystemMotion::Reduced => BlinkAnimation::None,
        }
    }
}

/// `[cursor] blink_interval_ms`: each phase's length when the config sets
/// it, which wins over the system's insertion-point keys; otherwise those
/// keys where the user set them, and 333 ms each where not (`caret.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlinkInterval {
    Default,
    Set(std::time::Duration),
}

/// When a blinking cursor stops blinking: never, the default, or after a
/// quiet time without input -- kitty's `cursor_stop_blinking_after 15.0`,
/// with a default of never here. Output never holds it off, so `tail -f`
/// stops blinking; it stops shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopAfter {
    Never,
    After(std::time::Duration),
}

/// `bell`: what a `BEL` does, the owner's call of 2026-10-13
/// ("System sound + Dock bounce"). **`Sound` by default**: `NSBeep`, the system's alert
/// sound, while the window is focused. `Flash` shows the visual bell `?5`'s
/// pair shows -- the other video for the entry's `flash` length -- and not
/// under Reduce Motion; `Nothing` neither. What an unfocused window does is
/// `?1042`'s and `?1043`'s, set by the application, and this does not
/// silence them (`bell.rs`, `ring`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BellStyle {
    #[default]
    Sound,
    Flash,
    Nothing,
}

/// Whether switching to or from the alternate screen fades the new screen
/// in from the background. Off by default, like the trail; a two-state enum
/// for the reason `CursorTrail` is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScreenTransition {
    #[default]
    Off,
    On,
}

/// Whether a trail in flight rides a smooth scroll with the text, or stays
/// where the screen had it. `Ride` by default, the owner's call of
/// 2026-09-25: a trail rides for at most two rows and then folds in on the
/// fast constant (`trail.rs`). `Stay` draws exactly what it drew before the
/// ride was built. Inert under `[scroll] mode = "normal"`, which slides
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailScroll {
    Ride,
    Stay,
}

/// Whether output scrolls the screen in one jump or slides it smoothly.
/// `Smooth` by default, matching sfte: the slide's added latency is bounded
/// to one row and an idle screen pays nothing, so it ships on rather than
/// behind an opt-in. A two-state enum for the reason `CursorTrail` is,
/// spelled `normal`/`smooth` rather than `true`/`false` because that is how
/// the setting reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScrollMode {
    Normal,
    #[default]
    Smooth,
}

/// The colour space the surface is tagged with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Space {
    #[default]
    Srgb,
    DisplayP3,
}

impl Config {
    /// The built-in defaults: the system mono, its colour emoji, and its
    /// symbols for the braille and box drawing Menlo lacks.
    pub fn defaults() -> Config {
        Config {
            faces: default_faces(),
            stack: StackFrom::Default,
            font_fallback: FontFallback::Embedded,
            font_size: 13.0,
            padding: 8.0,
            colorspace: Space::Srgb,
            theme: ThemeChoice::Auto,
            floor: baud_render::Floor::DEFAULT,
            palette_light: None,
            palette_dark: None,
            selection: Selection::default(),
            cursor: CursorColours::default(),
            cursor_trail: TrailPick::Default,
            cursor_shape: CursorShape::Block,
            cursor_blink: BlinkChoice::Auto,
            cursor_stop: StopAfter::Never,
            blink_animation: BlinkAnimation::Plasma,
            blink_interval: BlinkInterval::Default,
            cursor_guide: baud_domain::CursorGuide::Hidden,
            screen_transition: ScreenTransition::Off,
            exit_status: baud_render::StatusMarks::Off,
            badge: BadgeShown::On,
            checksum: baud_domain::ChecksumReport::On,
            logging: baud_domain::Logging::On,
            printing: Printing::On,
            notifications: Notifications::On,
            graphics_print: baud_domain::GraphicsPrint::DEFAULT,
            window_ops: baud_domain::WindowOps::On,
            pointer_mode: baud_domain::PointerMode::UnlessTracking,
            font_changes: baud_domain::FontChanges::On,
            scroll: ScrollMode::Smooth,
            trail_scroll: TrailScroll::Ride,
            glass: Glass::defaults(),
            images: crate::host::IMAGE_BUDGET,
            sixel: baud_domain::SixelClaim::No,
            vs16_width: baud_domain::Vs16Width::Two,
            ambiguous_width: baud_domain::AmbiguousWidth::Narrow,
            emoji: baud_render::EmojiDefault::Font,
            bell: BellStyle::Sound,
            clipboard: Clipboard::DEFAULT,
            config_reload: ConfigReload::On,
            source: None,
        }
    }

    /// The config from the first file that exists, over the defaults, as
    /// written: a family name not yet resolved.
    #[cfg(test)]
    fn read() -> Config {
        let (c, said) = Config::read_saying();
        for said in said {
            crate::warn!("{said}");
        }
        c
    }

    /// [`Config::read`], with what it could not act on returned rather
    /// than logged: a line that is not `key = value`, skipped as it always
    /// was (at launch no earlier config exists to keep), and each key
    /// [`Config::apply`] could not act on.
    fn read_saying() -> (Config, Vec<String>) {
        let mut c = Config::defaults();
        let mut said = Vec::new();
        if let Some((path, text)) = read_first() {
            if let Some(broken) = syntax(&text) {
                said.push(broken.said(&path, Fault::Skipped));
            }
            said.extend(c.apply(&text));
            c.beside(&path);
        }
        (c, said)
    }

    /// Resolve this config's family names to the installed faces, as
    /// [`Config::launch`] does, logging what it could not find: for a
    /// reload whose fonts changed.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn resolved(mut self) -> Config {
        for s in self.resolve(crate::discover::Catalogue::system()) {
            crate::warn!("{s}");
        }
        self
    }

    /// The paths in `self` made the file's at `path`: a relative font path
    /// beside it, and the file itself as the config in force.
    fn beside(&mut self, path: &std::path::Path) {
        if let Some(dir) = path.parent() {
            self.faces_beside(dir, absolute_dir(std::env::var_os("HOME")).as_deref());
        }
        self.source = Some(path.to_path_buf());
    }

    /// **The config at launch, and what it could not act on**: the config
    /// from the first file that exists, over the defaults -- the defaults
    /// themselves when none is found -- its family names resolved to the
    /// installed faces ([`Config::resolve`]); the sentences it logged, for
    /// the window to show (Jev's lean after a re-ask, LAUNCH_TOO, 0.78 and
    /// 0.84); and the files it pulls in, for the watch.
    pub fn launch() -> Launched {
        let (mut c, said) = Config::read_saying();
        for s in &said {
            crate::warn!("{s}");
        }
        let watched = c.watched();
        let read = c.clone();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            for s in c.resolve(crate::discover::Catalogue::system()) {
                crate::warn!("{s}");
            }
            crate::host::startup_mark("fonts named");
        }
        Launched {
            config: c,
            read,
            said,
            watched,
        }
    }

    /// **The config read again, for a running window** (`reload.rs`), its
    /// names not yet resolved. `in_force` is the file the window's config
    /// was read from, if one was.
    ///
    /// * A file that does not read as UTF-8 text, or has a line that is not
    ///   blank, a comment, a `[section]` header or `key = value`, or a list
    ///   it never closes, is [`Reread::Broken`]: the window keeps the last
    ///   good config (Jev's lean after a re-ask, SYNTAX_KEEPS, 0.82 and
    ///   0.77; alacritty keeps its config on a TOML error, WezTerm on any).
    /// * A key it cannot act on is applied as launch applies it, that key
    ///   at its default, and said.
    /// * The file in force gone, with none above it in the lookup, is
    ///   [`Reread::Gone`]: the window keeps the last config, as alacritty's
    ///   does. Jev leaned to running the lookup again (LOOKUP_AGAIN, 0.42
    ///   and 0.88), below the 0.9 a fall back to the defaults was asked to
    ///   carry.
    /// * An empty file is the defaults, as at launch (Jev's lean,
    ///   EMPTY_IS_DEFAULTS, 0.80 and 0.80): the watch's settle is what
    ///   keeps a save in progress from being read.
    /// * **The first file that exists is read**, where a launch reads the
    ///   first that reads: a file above the one in force that is not UTF-8
    ///   text, which a launch skips, is [`Reread::Broken`] here, and each
    ///   save of the file in force shows that until it is fixed or gone.
    pub fn reread(in_force: Option<&std::path::Path>) -> Reread {
        Config::reread_at(&lookup(), in_force)
    }

    /// [`Config::reread`] over the lookup `paths`, in their order.
    pub(crate) fn reread_at(
        paths: &[std::path::PathBuf],
        in_force: Option<&std::path::Path>,
    ) -> Reread {
        let first = paths.iter().position(|p| p.exists());
        let place = |p: &std::path::Path| paths.iter().position(|q| q == p).unwrap_or(usize::MAX);
        let gone = in_force.filter(|s| !s.exists() && first.is_none_or(|i| i > place(s)));
        if let Some(s) = gone {
            return Reread::Gone(s.to_path_buf());
        }
        let mut c = Config::defaults();
        let Some(path) = first.map(|i| paths[i].clone()) else {
            let watched = c.watched();
            return Reread::Read(Box::new(Reading {
                config: c,
                said: Vec::new(),
                watched,
            }));
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Reread::Broken(format!(
                    "{} is not UTF-8 text; the last good settings stay",
                    shown(&path)
                ));
            }
            Err(e) => {
                return Reread::Broken(format!(
                    "{} cannot be read ({e}); the last good settings stay",
                    shown(&path)
                ));
            }
        };
        if let Some(broken) = syntax(&text) {
            return Reread::Broken(broken.said(&path, Fault::Kept));
        }
        let said = c.apply(&text);
        c.beside(&path);
        let watched = c.watched();
        Reread::Read(Box::new(Reading {
            config: c,
            said,
            watched,
        }))
    }

    /// **The files this config pulls in, for the watch**: each place the
    /// config is looked up, each path its theme names are tried at, and
    /// each font it names by path. Before [`Config::resolve`], which turns
    /// a family name into a file too.
    fn watched(&self) -> Watched {
        let dirs = crate::palette::Dirs::from_env(self.source.as_deref());
        let themes = [&self.palette_light, &self.palette_dark]
            .into_iter()
            .flatten()
            .flat_map(|spec| crate::palette::candidates(spec, &dirs))
            .collect();
        let faces = match self.stack {
            StackFrom::Configured => self
                .faces
                .iter()
                .filter(|f| f.from == FaceFrom::File)
                .map(|f| std::path::PathBuf::from(&f.path))
                .collect(),
            StackFrom::Default => Vec::new(),
        };
        Watched {
            lookup: lookup(),
            themes,
            faces,
        }
    }

    /// **The stack's family names, made files**: each name the installed
    /// face `cat` picks for its regular style (`[[source: css-fonts-3]]`
    /// 5.2, [`crate::discover::pick`]), and a file that does not exist
    /// left out. Once at launch, off every per-byte and per-frame path.
    /// **The first font of a config's list that is found brings its
    /// family's styled faces**:
    /// its Bold, Italic and Bold Italic, each the face CSS 5.2 picks for the
    /// style in the family it names (found among the installed faces, and
    /// for a font named by path beside its file too), left out where that
    /// pick is the regular face itself -- so a family without a bold draws
    /// bold from its regular face, never a synthesised one (Jev, R_REGULAR,
    /// 0.98/0.98), and one without italics draws bold italic from its Bold
    /// (B_CSS, Jev's lean). The default stack's styled faces are the
    /// embedded ones, as they were.
    ///
    /// Returns what it could not act on: a name a config gave that is no
    /// installed family, and a path that is no file, each left out of the
    /// stack; and, when nothing a config named is left, the built-in stack
    /// in its place. The defaults' own misses are left out without a word,
    /// as a default path that did not exist always was.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn resolve(&mut self, cat: &crate::discover::Catalogue) -> Vec<String> {
        use crate::discover::{Picked, Want, pick};
        let mut said = Vec::new();
        let configured = self.stack == StackFrom::Configured;
        let mut kept = Vec::new();
        // Whether a face the config named was found: the embedded faces
        // behind a configured stack are not one, and a name can now pick
        // an embedded face, so the kept faces' origins cannot say.
        let mut found = Found::None;
        // The first face a config named that was found, and its family's
        // other styles: CSS 5.2's pick of each in the family it names.
        let mut styled: Option<Vec<FaceSpec>> = None;
        let styles_of = |family: &[&crate::discover::Known], regular: &Picked| {
            use baud_render::FaceStyle;
            [
                (Want::BOLD, FaceStyle::Bold),
                (Want::ITALIC, FaceStyle::Italic),
                (Want::BOLD_ITALIC, FaceStyle::BoldItalic),
            ]
            .into_iter()
            .filter_map(|(want, style)| {
                let p = pick(family, want).filter(|p| p != regular)?;
                Some(FaceSpec::picked(&p, baud_render::FaceRole::Style(style)))
            })
            .collect::<Vec<_>>()
        };
        for mut f in std::mem::take(&mut self.faces) {
            match f.from {
                FaceFrom::Embedded(_) => kept.push(f),
                FaceFrom::File => {
                    if std::fs::metadata(&f.path).is_ok_and(|m| m.is_file()) {
                        if configured && styled.is_none() {
                            // Drawn at the regular style's axis values, as
                            // the face named by its family is, so a style
                            // whose pick is this face is told apart from it.
                            let beside = cat.beside(&f.path, f.index);
                            if let Some(p) = beside.regular() {
                                f.variations = p.variations.clone();
                            }
                            let regular = Picked {
                                path: f.path.clone(),
                                origin: crate::discover::Origin::File,
                                index: f.index,
                                variations: f.variations.clone(),
                            };
                            styled = Some(styles_of(&beside.faces(), &regular));
                        } else if let Some(p) = crate::discover::regular_of(&f.path, f.index) {
                            // A later font, or one of the defaults: drawn at
                            // the regular style's axis values all the same.
                            f.variations = p.variations;
                        }
                        found = Found::Some;
                        kept.push(f);
                    } else if configured {
                        said.push(format!(
                            "config: font {:?} is not a file; the stack goes on without it",
                            f.path
                        ));
                    }
                }
                FaceFrom::Family => {
                    use crate::discover::Named;
                    // The face a name picks, and the faces its other
                    // styles are picked among: a family's, or for a face
                    // named alone, its family's.
                    let (regular, family) = match cat.lookup(&f.path) {
                        Named::Face(k) => (pick(&[k], Want::REGULAR), cat.family_of(k)),
                        Named::Family(fam) => (pick(&fam, Want::REGULAR), fam),
                        Named::Nothing => (None, Vec::new()),
                    };
                    match regular {
                        Some(p) => {
                            if configured && styled.is_none() {
                                styled = Some(styles_of(&family, &p));
                            }
                            found = Found::Some;
                            kept.push(FaceSpec::picked(&p, f.role));
                        }
                        None if configured => said.push(format!(
                            "config: no installed font is named {:?}; the stack goes on without it",
                            f.path
                        )),
                        None => {}
                    }
                }
            }
        }
        if configured && found == Found::None {
            said.push(
                "config: no font fonts names was found; the built-in stack is used instead".into(),
            );
            self.faces = default_faces();
            self.stack = StackFrom::Default;
            said.extend(self.resolve(cat));
            return said;
        }
        kept.extend(styled.unwrap_or_default());
        self.faces = kept;
        said
    }

    /// **A relative font path names a file beside the config**, in `dir`,
    /// the directory the config file is in -- the owner's call of
    /// 2026-10-13 -- and not under the directory baud was started in, which
    /// from the Dock is `/`. **One that starts `~/` names a file in `home`**,
    /// the home folder, as a theme path does ([`crate::palette::candidates`])
    /// and as ghostty expands one (`[[source: ghostty-d9840f3-path-zig]]`:
    /// "We explicitly look for \"~/\" because we don't support alternate
    /// users such as \"~alice/\""), so `~user/` is relative like any other
    /// name; with no home folder it is kept as written, and the launch
    /// says it is not a file. An absolute path and a compiled-in face are
    /// kept. The `@N` of a collection was split off when the value was
    /// read, so only the path is joined. A path that `dir` would make
    /// unreadable as UTF-8 is kept as written.
    fn faces_beside(&mut self, dir: &std::path::Path, home: Option<&std::path::Path>) {
        for f in &mut self.faces {
            if f.from != FaceFrom::File || !std::path::Path::new(&f.path).is_relative() {
                continue;
            }
            let joined = match f.path.strip_prefix("~/") {
                Some(rest) => match home {
                    Some(h) => h.join(rest),
                    None => continue,
                },
                None => dir.join(&f.path),
            };
            if let Some(joined) = joined.to_str() {
                f.path = joined.to_string();
            }
        }
    }

    /// Reads `text` over `self`, and returns what it could not act on, one
    /// sentence each: a key no arm knows in the section it is in, and a
    /// value that fell back to a default. `load` prints them; a test reads
    /// them, because a config whose every value equals its default reads
    /// the same whether a line landed or was swallowed.
    pub(crate) fn apply(&mut self, text: &str) -> Vec<String> {
        let mut said = Vec::new();
        // The stack is settled after the whole file, so `font_fallback`
        // counts wherever it sits relative to `fonts`.
        let mut named: Option<Vec<FaceSpec>> = None;
        for (section, key, value) in fields(text) {
            match (section.as_str(), key.as_str()) {
                ("", "font_size") => match value.trim().parse::<f32>() {
                    Ok(v) => self.font_size = v,
                    Err(_) => said.push(format!(
                        "config: font_size {:?} is not a number; keeping the default",
                        value.trim()
                    )),
                },
                // Recognised only to say it is gone. Dropping it to the
                // wildcard would leave the owner's own config carrying a
                // number that reaches no drawing code, which is the
                // failure the example's `GN_GTINT` comment records: a
                // knob that reports one thing and does another.
                ("", "background_alpha") => {
                    said.push(
                        "config: background_alpha is superseded by [glass] dim_dark and \
                         dim_light, which change with the system appearance; it is ignored"
                            .into(),
                    );
                }
                ("", "padding") => match value.trim().parse::<f32>() {
                    Ok(v) => self.padding = v.max(0.0),
                    Err(_) => said.push(format!(
                        "config: padding {:?} is not a number; keeping the default",
                        value.trim()
                    )),
                },
                ("", "theme") => {
                    self.theme = match value.trim().trim_matches('"') {
                        "auto" => ThemeChoice::Auto,
                        "dark" => ThemeChoice::Dark,
                        "light" => ThemeChoice::Light,
                        other => {
                            said.push(format!(
                                "config: theme {other:?} is not auto, dark or light; using auto"
                            ));
                            ThemeChoice::Auto
                        }
                    };
                }
                ("", "emoji") => {
                    self.emoji = match value.trim().trim_matches('"') {
                        "font" => baud_render::EmojiDefault::Font,
                        "system" => baud_render::EmojiDefault::System,
                        other => {
                            said.push(format!(
                                "config: emoji {other:?} is not font or system; using font"
                            ));
                            baud_render::EmojiDefault::Font
                        }
                    };
                }
                ("", "config_reload") => {
                    self.config_reload = match value.trim().trim_matches('"') {
                        "on" => ConfigReload::On,
                        "off" => ConfigReload::Off,
                        other => {
                            said.push(format!(
                                "config: config_reload {other:?} is not on or off; using on"
                            ));
                            ConfigReload::On
                        }
                    };
                }
                ("", "bell") => {
                    self.bell = match value.trim().trim_matches('"') {
                        "sound" => BellStyle::Sound,
                        "flash" => BellStyle::Flash,
                        "none" => BellStyle::Nothing,
                        other => {
                            said.push(format!(
                                "config: bell {other:?} is not sound, flash or none; using sound"
                            ));
                            BellStyle::Sound
                        }
                    };
                }
                ("", "vs16_width") => {
                    self.vs16_width = match value.trim().trim_matches('"') {
                        "two" => baud_domain::Vs16Width::Two,
                        "one" => baud_domain::Vs16Width::One,
                        other => {
                            said.push(format!(
                                "config: vs16_width {other:?} is not two or one; using two"
                            ));
                            baud_domain::Vs16Width::Two
                        }
                    };
                }
                // Recognised only to say it is gone: text is drawn by one
                // model on every system now, DirectWrite's grayscale contrast
                // curve, and a switch that reached no drawing code would be a
                // knob that reports one thing and does another.
                ("", "font_smoothing") => {
                    said.push(
                        "config: font_smoothing is gone -- baud draws text one way on every \
                         system; it is ignored"
                            .into(),
                    );
                }
                ("", "colorspace") => {
                    self.colorspace = match value.trim().trim_matches('"') {
                        "srgb" => Space::Srgb,
                        "display-p3" => Space::DisplayP3,
                        other => {
                            said.push(format!(
                                "config: colorspace {other:?} is not srgb or display-p3; using srgb"
                            ));
                            Space::Srgb
                        }
                    };
                }
                ("palette", "floor") => self.floor = floor(&value, &mut said),
                ("palette", "light") => {
                    self.palette_light = Some(value.trim().trim_matches('"').to_string())
                }
                ("palette", "dark") => {
                    self.palette_dark = Some(value.trim().trim_matches('"').to_string())
                }
                ("palette", "selection") => {
                    self.selection.page = selection_pick(&value, "selection", &mut said)
                }
                ("palette", "cursor") => {
                    self.cursor.fill = selection_pick(&value, "cursor", &mut said)
                }
                ("palette", "cursor_text") => {
                    self.cursor.text = selection_pick(&value, "cursor_text", &mut said)
                }
                ("palette", "selection_text") => {
                    self.selection.text = selection_pick(&value, "selection_text", &mut said)
                }
                ("cursor", "trail") => {
                    // `true` is the default's word and not a name: it has
                    // meant "the default" since the key was a switch.
                    let named = |t| TrailPick::Named(t);
                    self.cursor_trail = match value.trim().trim_matches('"') {
                        "true" => TrailPick::Default,
                        "fire" => named(CursorTrail::Fire),
                        "plasma" => named(CursorTrail::Plasma),
                        "lightning" => named(CursorTrail::Lightning),
                        "ice" => named(CursorTrail::Ice),
                        "ufo" => named(CursorTrail::Ufo),
                        "orb" => named(CursorTrail::Orb),
                        "liquid" => named(CursorTrail::Liquid),
                        "quad" => named(CursorTrail::Quad),
                        "sweep" => named(CursorTrail::Sweep),
                        "ghost" => named(CursorTrail::Ghost),
                        "off" | "false" => named(CursorTrail::Off),
                        other => {
                            said.push(format!(
                                "config: [cursor] trail {other:?} is not fire, plasma, lightning, ice, ufo, orb, \
                                 liquid, quad, sweep, ghost or off; using the default"
                            ));
                            TrailPick::Default
                        }
                    };
                }
                ("cursor", "shape") => {
                    self.cursor_shape = match value.trim().trim_matches('"') {
                        "block" => CursorShape::Block,
                        "underline" => CursorShape::Underline,
                        "bar" => CursorShape::Bar,
                        other => {
                            said.push(format!(
                                "config: [cursor] shape {other:?} is not block, underline \
                                 or bar; using block"
                            ));
                            CursorShape::Block
                        }
                    };
                }
                ("cursor", "blink") => {
                    self.cursor_blink = match value.trim().trim_matches('"') {
                        "auto" => BlinkChoice::Auto,
                        "true" => BlinkChoice::On,
                        "false" => BlinkChoice::Off,
                        other => {
                            said.push(format!(
                                "config: [cursor] blink {other:?} is not auto, true or false; \
                                 using auto"
                            ));
                            BlinkChoice::Auto
                        }
                    };
                }
                ("cursor", "stop_blinking_after") => {
                    let secs = value.trim().parse::<f64>().ok();
                    self.cursor_stop = match secs {
                        Some(0.0) => StopAfter::Never,
                        Some(s) => match std::time::Duration::try_from_secs_f64(s) {
                            Ok(d) => StopAfter::After(d),
                            Err(_) => {
                                said.push(format!(
                                    "config: [cursor] stop_blinking_after {s} is not a time; never"
                                ));
                                StopAfter::Never
                            }
                        },
                        None => {
                            said.push(format!(
                                "config: [cursor] stop_blinking_after {:?} is not a number of \
                                 seconds; never",
                                value.trim()
                            ));
                            StopAfter::Never
                        }
                    };
                }
                ("cursor", "blink_animation") => {
                    self.blink_animation = match value.trim().trim_matches('"') {
                        "plasma" => BlinkAnimation::Plasma,
                        "fade" => BlinkAnimation::Fade,
                        "fire" => BlinkAnimation::Fire,
                        "ufo" => BlinkAnimation::Ufo,
                        "none" => BlinkAnimation::None,
                        other => {
                            said.push(format!(
                                "config: [cursor] blink_animation {other:?} is not plasma, fade, \
                                 fire, ufo or none; using the default, plasma"
                            ));
                            BlinkAnimation::Plasma
                        }
                    };
                }
                ("cursor", "guide") => {
                    self.cursor_guide = match value.trim().trim_matches('"') {
                        "on" => baud_domain::CursorGuide::Shown,
                        "off" => baud_domain::CursorGuide::Hidden,
                        other => {
                            said.push(format!(
                                "config: [cursor] guide {other:?} is not on or off; using off"
                            ));
                            baud_domain::CursorGuide::Hidden
                        }
                    };
                }
                ("cursor", "blink_interval_ms") => {
                    let ms = value.trim().parse::<f64>().ok();
                    // A positive number past the ceiling is clamped to it, as
                    // every phase is, rather than refused.
                    let most = crate::caret::CEILING.as_secs_f64() * 1000.0;
                    self.blink_interval = match ms
                        .filter(|ms| ms.is_finite() && *ms > 0.0)
                        .and_then(|ms| {
                            std::time::Duration::try_from_secs_f64(ms.min(most) / 1000.0).ok()
                        }) {
                        Some(d) => BlinkInterval::Set(d),
                        None => {
                            said.push(format!(
                                "config: [cursor] blink_interval_ms {:?} is not a positive \
                                 number of milliseconds; using the default, 333",
                                value.trim()
                            ));
                            BlinkInterval::Default
                        }
                    };
                }
                ("screen", "transition") => {
                    self.screen_transition = match value.trim() {
                        "true" => ScreenTransition::On,
                        "false" => ScreenTransition::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] transition {other:?} is not true or false; \
                                 using false"
                            ));
                            ScreenTransition::Off
                        }
                    };
                }
                ("screen", "badge") => {
                    self.badge = match value.trim().trim_matches('"') {
                        "on" => BadgeShown::On,
                        "off" => BadgeShown::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] badge {other:?} is not on or off; using on"
                            ));
                            BadgeShown::On
                        }
                    };
                }
                ("screen", "exit_status") => {
                    self.exit_status = match value.trim().trim_matches('"') {
                        "on" => baud_render::StatusMarks::On,
                        "off" => baud_render::StatusMarks::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] exit_status {other:?} is not on or off; \
                                 using off"
                            ));
                            baud_render::StatusMarks::Off
                        }
                    };
                }
                ("screen", "checksum") => {
                    self.checksum = match value.trim().trim_matches('"') {
                        "on" => baud_domain::ChecksumReport::On,
                        "off" => baud_domain::ChecksumReport::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] checksum {other:?} is not on or off; \
                                 using on"
                            ));
                            baud_domain::ChecksumReport::On
                        }
                    };
                }
                ("screen", "logging") => {
                    self.logging = match value.trim().trim_matches('"') {
                        "on" => baud_domain::Logging::On,
                        "off" => baud_domain::Logging::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] logging {other:?} is not on or off; \
                                 using on"
                            ));
                            baud_domain::Logging::On
                        }
                    };
                }
                ("screen", "printing") => {
                    self.printing = match value.trim().trim_matches('"') {
                        "on" => Printing::On,
                        "off" => Printing::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] printing {other:?} is not on or off; \
                                 using on"
                            ));
                            Printing::On
                        }
                    };
                }
                ("screen", "notifications") => {
                    self.notifications = match value.trim().trim_matches('"') {
                        "on" => Notifications::On,
                        "off" => Notifications::Off,
                        other => {
                            said.push(format!(
                                "config: [screen] notifications {other:?} is not on or off; \
                                 using on"
                            ));
                            Notifications::On
                        }
                    };
                }
                ("screen", "print_colour") => {
                    self.graphics_print.colour = match on_off("print_colour", &value, &mut said) {
                        true => baud_domain::PrintColour::Colour,
                        false => baud_domain::PrintColour::Mono,
                    }
                }
                ("screen", "print_rgb") => {
                    self.graphics_print.syntax = match on_off("print_rgb", &value, &mut said) {
                        true => baud_domain::PrintSyntax::Rgb,
                        false => baud_domain::PrintSyntax::Hls,
                    }
                }
                ("screen", "print_background") => {
                    self.graphics_print.background =
                        match on_off("print_background", &value, &mut said) {
                            true => baud_domain::PrintBackground::On,
                            false => baud_domain::PrintBackground::Off,
                        }
                }
                ("screen", "print_rotated") => {
                    self.graphics_print.rotation = match on_off("print_rotated", &value, &mut said) {
                        true => baud_domain::PrintRotation::Rotated,
                        false => baud_domain::PrintRotation::Upright,
                    }
                }
                ("window", "operations") => {
                    self.window_ops = match value.trim().trim_matches('"') {
                        "on" => baud_domain::WindowOps::On,
                        "off" => baud_domain::WindowOps::Off,
                        other => {
                            said.push(format!(
                                "config: [window] operations {other:?} is not on or off; \
                                 using on"
                            ));
                            baud_domain::WindowOps::On
                        }
                    };
                }
                ("font", "changes") => {
                    self.font_changes = match value.trim().trim_matches('"') {
                        "on" => baud_domain::FontChanges::On,
                        "off" => baud_domain::FontChanges::Off,
                        other => {
                            said.push(format!(
                                "config: [font] changes {other:?} is not on or off; using on"
                            ));
                            baud_domain::FontChanges::On
                        }
                    };
                }
                ("pointer", "hide_while_typing") => {
                    self.pointer_mode = match value.trim().trim_matches('"') {
                        "on" => baud_domain::PointerMode::UnlessTracking,
                        "off" => baud_domain::PointerMode::Never,
                        other => {
                            said.push(format!(
                                "config: [pointer] hide_while_typing {other:?} is not on or \
                                 off; using on"
                            ));
                            baud_domain::PointerMode::UnlessTracking
                        }
                    };
                }
                // Quoted, as the example writes it, or bare: the example's
                // `"normal"` was read as smooth until the example's own gate
                // asked the reader what it had ignored.
                ("cursor", "trail_scroll") => {
                    self.trail_scroll = match value.trim().trim_matches('"') {
                        "ride" => TrailScroll::Ride,
                        "stay" => TrailScroll::Stay,
                        other => {
                            said.push(format!(
                                "config: [cursor] trail_scroll {other:?} is not ride or stay; \
                                 using the default, ride"
                            ));
                            TrailScroll::Ride
                        }
                    };
                }
                ("scroll", "mode") => {
                    self.scroll = match value.trim().trim_matches('"') {
                        "smooth" => ScrollMode::Smooth,
                        "normal" => ScrollMode::Normal,
                        other => {
                            said.push(format!(
                                "config: [scroll] mode {other:?} is not normal or smooth; \
                                 using the default, smooth"
                            ));
                            ScrollMode::Smooth
                        }
                    };
                }
                // The image arena, in the units a reader thinks in: MiB of
                // decoded ARGB pixels. Converted to the store's own unit
                // (`ColorWord`s) here as a local -- a `usize` field on `Config`
                // would trip the audit, but `ImageBudget` is a `baud-domain`
                // type and exempt.
                ("images", "pixels_mb") => match value.trim().parse::<usize>() {
                    Ok(mb) => self.images.pixels = mb.saturating_mul(1 << 18),
                    Err(_) => warn_size(&value, "pixels_mb", &mut said),
                },
                // The raw-Sixel scratch this sized until 2026-10-14, when a
                // Sixel began decoding as it arrived: a config that still
                // names it is told so, by name, rather than with the line an
                // unknown key gets, which would read as a typo.
                ("images", "body_kb") => said.push(
                    "config: [images] body_kb no longer limits anything: a Sixel is \
                     decoded as it arrives and no body is held; pixels_mb is the limit"
                        .into(),
                ),
                ("images", "advertise_sixel") => {
                    self.sixel = match value.trim() {
                        "true" => baud_domain::SixelClaim::Yes,
                        "false" => baud_domain::SixelClaim::No,
                        other => {
                            said.push(format!(
                                "config: [images] advertise_sixel = {other:?} is not true or false; it stays off"
                            ));
                            baud_domain::SixelClaim::No
                        }
                    };
                }
                ("text", "ambiguous_width") => {
                    self.ambiguous_width = match value.trim().trim_matches('"') {
                        "narrow" => baud_domain::AmbiguousWidth::Narrow,
                        "wide" => baud_domain::AmbiguousWidth::Wide,
                        other => {
                            said.push(format!(
                                "config: [text] ambiguous_width {other:?} is not narrow or wide; \
                                 using the default, narrow"
                            ));
                            baud_domain::AmbiguousWidth::Narrow
                        }
                    };
                }
                ("clipboard", "osc52") => {
                    self.clipboard.osc52 = match value.trim().trim_matches('"') {
                        "write" => Osc52::Write,
                        "off" => Osc52::Off,
                        other => {
                            said.push(format!(
                                "config: [clipboard] osc52 {other:?} is not write or off; \
                                 using the default, write"
                            ));
                            Osc52::Write
                        }
                    };
                }
                ("clipboard", "copy_on_select") => {
                    self.clipboard.copy_on_select = match value.trim().trim_matches('"') {
                        "on" => CopyOnSelect::On,
                        "off" => CopyOnSelect::Off,
                        other => {
                            said.push(format!(
                                "config: [clipboard] copy_on_select {other:?} is not on or off; \
                                 using the default, on"
                            ));
                            CopyOnSelect::On
                        }
                    };
                }
                // KiB of base64 body: the budget is a `baud-domain` type, so
                // no `usize` field lands on `Config`.
                ("clipboard", "body_kb") => match value.trim().parse::<usize>() {
                    Ok(kb) => {
                        self.clipboard.budget =
                            baud_domain::ClipboardBudget::of_body(kb.saturating_mul(1 << 10));
                    }
                    Err(_) => said.push(format!(
                        "config: [clipboard] body_kb {:?} is not a whole number; keeping the default",
                        value.trim()
                    )),
                },
                ("clipboard", other) => {
                    said.push(format!(
                        "config: [clipboard] has no key {other:?}; it is ignored"
                    ));
                }
                ("images", other) => {
                    said.push(format!(
                        "config: [images] has no key {other:?}; it is ignored"
                    ));
                }
                ("", "fonts") => {
                    let faces: Vec<_> =
                        strings(&value).iter().map(|s| FaceSpec::parse(s)).collect();
                    if faces.is_empty() {
                        said.push("config: fonts names no face; keeping the default stack".into());
                    } else {
                        named = Some(faces);
                    }
                }
                ("", "font_fallback") => {
                    self.font_fallback = match value.trim() {
                        "true" => FontFallback::Embedded,
                        "false" => FontFallback::Off,
                        other => {
                            said.push(format!(
                                "config: font_fallback = {other:?} is not true or false; the embedded faces stay behind the stack"
                            ));
                            FontFallback::Embedded
                        }
                    };
                }
                ("glass", "style") => {
                    self.glass.style = match value.trim().trim_matches('"') {
                        "standard" => Material::Standard,
                        "clear" => Material::Clear,
                        "regular" => Material::Regular,
                        other => {
                            said.push(format!(
                                "config: glass style {other:?} is not standard, regular or clear; \
                                 using standard"
                            ));
                            Glass::defaults().style
                        }
                    };
                }
                ("glass", "shadow") => {
                    self.glass.shadow = match value.trim().trim_matches('"') {
                        "auto" => Shadow::Auto,
                        "always" => Shadow::Always,
                        "never" => Shadow::Never,
                        other => {
                            said.push(format!(
                                "config: glass shadow {other:?} is not auto, always or \
                                 never; using auto"
                            ));
                            Shadow::Auto
                        }
                    };
                }
                ("glass", "app_backgrounds") => {
                    self.glass.app_backgrounds =
                        app_backgrounds(&value, "app_backgrounds", &mut said);
                }
                // The key's first name, shipped for one day as a
                // block-elements-only rule. Applied under its new meaning
                // rather than dropped, and said, so a config written for it
                // keeps working and learns the name that will stay.
                ("glass", "images") => {
                    said.push(
                        "config: glass images is now app_backgrounds, and \"opaque\" keeps \
                         every background an application sets, not only block elements"
                            .into(),
                    );
                    self.glass.app_backgrounds = app_backgrounds(&value, "images", &mut said);
                }
                ("glass", "pictures") => self.glass.pictures = pictures(&value, &mut said),
                ("glass", "dim_dark") => self.glass.dim_dark = unit(&value, "dim_dark", &mut said),
                ("glass", "dim_light") => {
                    self.glass.dim_light = unit(&value, "dim_light", &mut said)
                }
                ("glass", "tint_dark") => {
                    self.glass.tint_dark = unit(&value, "tint_dark", &mut said)
                }
                ("glass", "tint_light") => {
                    self.glass.tint_light = unit(&value, "tint_light", &mut said)
                }
                ("glass", "hue") => self.glass.hue = unit(&value, "hue", &mut said),
                ("glass", "saturation") => {
                    self.glass.saturation = unit(&value, "saturation", &mut said)
                }
                // `[glass]` is a closed set of ten, so a name that is
                // not one of them is a typo rather than a key from a
                // newer baud, and saying so costs a line. It is a warning
                // and not an error for the reason every other unknown key
                // is ignored: a newer config must not break an older
                // binary.
                ("glass", other) => {
                    said.push(format!(
                        "config: [glass] has no key {other:?}; it is ignored"
                    ));
                }
                ("", other) => said.push(format!("config: no key {other:?}; it is ignored")),
                (section, other) => said.push(format!(
                    "config: [{section}] has no key {other:?}; it is ignored"
                )),
            }
        }
        if let Some(mut faces) = named {
            // The owner's order: the configured faces, then baud's embedded
            // font, then the system, which is not built for a configured
            // stack yet (note AC decision 9). The embedded Regular goes first
            // of the three, so it draws whatever the named faces lack and it
            // maps: the 52 Hebrew code points, which were `.notdef`, and
            // whatever else JetBrains Mono maps. By cmap, not rendering, that
            // is 458 code points behind
            // `baud-font/tests/fonts/FiraCode-Regular.ttf`: the 52, 360 that
            // drew from STIX before, 8 from the icon face (U+23FB-23FE,
            // U+26A1, U+276E-276F, U+2B58) and 38 more that were boxes.
            // Regular only: a styled face is not in the chain, so the bold
            // and italic Hebrew a configured family's styled faces lack
            // draws from this face, upright and regular weight.
            if self.font_fallback == FontFallback::Embedded {
                faces.push(FaceSpec::embedded(crate::embedded::REGULAR));
                faces.push(FaceSpec::embedded(crate::embedded::SYMBOLS));
                faces.push(FaceSpec::embedded(crate::embedded::STIX));
            }
            // The badge's face, whatever the text's and with or without the
            // fallback: not in the chain, it draws no text.
            faces.push(FaceSpec::badge());
            self.faces = faces;
            self.stack = StackFrom::Configured;
        }
        said
    }
}

/// `[glass] app_backgrounds`'s value, or `dim` with a warning on a word it
/// does not know.
fn app_backgrounds(value: &str, key: &str, said: &mut Vec<String>) -> baud_render::AppBg {
    match value.trim().trim_matches('"') {
        "dim" => baud_render::AppBg::Dimmed,
        "opaque" => baud_render::AppBg::Opaque,
        other => {
            said.push(format!(
                "config: glass {key} {other:?} is not dim or opaque; using dim"
            ));
            baud_render::AppBg::Dimmed
        }
    }
}

/// `[glass] pictures`'s value, or `opaque` with a warning on a word it does
/// not know.
fn pictures(value: &str, said: &mut Vec<String>) -> baud_render::PictureBg {
    match value.trim().trim_matches('"') {
        "opaque" => baud_render::PictureBg::Opaque,
        "dim" => baud_render::PictureBg::Dimmed,
        other => {
            said.push(format!(
                "config: glass pictures {other:?} is not opaque or dim; using opaque"
            ));
            baud_render::PictureBg::Opaque
        }
    }
}

/// `[palette] floor`: the contrast floor's trigger, 1.0 to 21.0, or `"off"`.
/// Its own parser, because `unit` clamps to 0-1 and would turn 1.2 into 1.0,
/// which is off. **The target is the larger of 4.5 and the trigger**, so a
/// reader asking for 7 (WCAG AAA) gets text lifted to 7, never to a ratio
/// below the one they set; at or below 4.5 it is the owner's 4.5 (note Y,
/// decision 6). A value that does not parse keeps the default and says so.
fn floor(value: &str, said: &mut Vec<String>) -> baud_render::Floor {
    let raw = value.trim().trim_matches('"');
    if raw == "off" {
        return baud_render::Floor::Off;
    }
    match raw.parse::<f64>() {
        Ok(t) if (1.0..=21.0).contains(&t) => baud_render::Floor::Lift {
            trigger: t,
            target: t.max(4.5),
        },
        _ => {
            said.push(format!(
                "config: palette floor {raw:?} is not \"off\" or a ratio from 1 to 21; using 1.2"
            ));
            baud_render::Floor::DEFAULT
        }
    }
}

/// One of `[palette]`'s selection or cursor colours: `"none"`, kitty's word
/// for no colour of its own, or a colour as an `OSC` names one (`XColor::parse`:
/// `#rrggbb`, `rgb:`, an X11 name). Anything else is said and leaves the
/// theme's.
fn selection_pick(value: &str, key: &str, said: &mut Vec<String>) -> SelectionPick {
    let raw = value.trim().trim_matches('"');
    if raw == "none" {
        return SelectionPick::None;
    }
    match baud_domain::XColor::parse(raw.as_bytes()) {
        baud_domain::XColor::Rgb(r, g, b) => SelectionPick::Colour(baud_render::Argb::rgb(r, g, b)),
        _ => {
            said.push(format!(
                "config: palette {key} {raw:?} is not \"none\" or a colour; the theme's stays"
            ));
            SelectionPick::Theme
        }
    }
}

/// **`[palette] selection` and `selection_text`, the `Theme` selection
/// setting**: the page under selected text and the selected text, over both
/// halves. Each wins over a theme file's colour, as a key set after the
/// theme wins in ghostty and kitty (Jev, 0.97 both orders); both unset and
/// no theme colour is reverse video, the default (the owner's V13 call).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Selection {
    pub page: SelectionPick,
    pub text: SelectionPick,
}

/// **`[palette] cursor` and `cursor_text`**: the cursor's colour and the
/// text under a block cursor, over both halves, each winning over a theme
/// file's as the selection's do (Jev's lean, 0.74 and 0.85, for these keys
/// in `[palette]`). A program's `OSC 12` wins over the colour while set and
/// leaves the text colour as it is; `OSC 112` returns to the colour here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CursorColours {
    pub fill: SelectionPick,
    pub text: SelectionPick,
}

/// One selection or cursor colour as the config names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectionPick {
    /// Not named: the theme file's colour, or none.
    #[default]
    Theme,
    /// `"none"`: no colour of its own, whatever the theme file says (Jev,
    /// 0.98 and 1.00 for kitty's word).
    None,
    /// A colour, whatever the theme file says.
    Colour(baud_render::Argb),
}

impl SelectionPick {
    /// The colour drawn, given the theme file's.
    pub fn over(
        self,
        theme: Option<baud_domain::themefile::CellPaint<baud_render::Argb>>,
    ) -> Option<baud_domain::themefile::CellPaint<baud_render::Argb>> {
        match self {
            SelectionPick::Theme => theme,
            SelectionPick::None => None,
            SelectionPick::Colour(c) => Some(baud_domain::themefile::CellPaint::Colour(c)),
        }
    }
}

/// A `[glass]` fraction: parsed, and clamped to 0..=1 with a warning.
///
/// Clamped rather than rejected, for the reason a malformed `GN_ORIGIN` is
/// ignored rather than fatal in the example: a mistyped alpha should not
/// stop the window appearing. It says what it did, because a value
/// silently changed is the same defect as a key silently ignored.
fn unit(value: &str, name: &str, said: &mut Vec<String>) -> f32 {
    let raw = value.trim();
    let Ok(v) = raw.parse::<f32>() else {
        let d = fallback(name);
        said.push(format!(
            "config: glass {name} {raw:?} is not a number; using {d}"
        ));
        return d;
    };
    if !(0.0..=1.0).contains(&v) {
        let c = v.clamp(0.0, 1.0);
        said.push(format!(
            "config: glass {name} {v} is outside 0..1; using {c}"
        ));
        return c;
    }
    v
}

/// Said when a `[images]` size does not parse: the knob keeps its default,
/// the way a malformed `font_size` does -- a mistyped memory budget must not
/// stop the window appearing -- and it says so, because a value silently
/// ignored is the same defect as a key silently ignored.
///
/// Takes `(value, name)` in `unit`'s order, deliberately: two adjacent
/// `&str`-pair helpers with swapped orders is a transposition the compiler
/// cannot catch.
fn warn_size(value: &str, name: &str, said: &mut Vec<String>) {
    said.push(format!(
        "config: [images] {name} {:?} is not a whole number; keeping the default",
        value.trim()
    ));
}

/// What a rejected `[glass]` value falls back to: the default for that key
/// rather than a shared zero, which would turn a typo into a missing
/// material instead of a default one.
fn fallback(name: &str) -> f32 {
    let g = Glass::defaults();
    match name {
        "dim_dark" => g.dim_dark,
        "dim_light" => g.dim_light,
        "tint_dark" => g.tint_dark,
        "tint_light" => g.tint_light,
        "hue" => g.hue,
        _ => g.saturation,
    }
}

impl FaceSpec {
    /// A `fonts` value: a file, when [`names_a_file`] says so, with any
    /// `@N`; else a family name, trimmed.
    fn parse(s: &str) -> FaceSpec {
        let fallback = |path: &str, index, from| FaceSpec {
            path: path.to_string(),
            index,
            from,
            role: baud_render::FaceRole::Fallback,
            variations: Vec::new(),
        };
        if !names_a_file(s) {
            return fallback(s.trim(), baud_font::FaceIndex::FIRST, FaceFrom::Family);
        }
        match s
            .rsplit_once('@')
            .and_then(|(p, i)| Some((p, baud_font::FaceIndex::of(i.parse().ok()?))))
        {
            Some((path, index)) => fallback(path, index, FaceFrom::File),
            None => fallback(s, baud_font::FaceIndex::FIRST, FaceFrom::File),
        }
    }

    /// A font compiled into the binary, in the fallback chain.
    fn embedded(e: crate::embedded::Embedded) -> FaceSpec {
        FaceSpec {
            path: e.name.to_string(),
            index: baud_font::FaceIndex::FIRST,
            from: FaceFrom::Embedded(e.bytes),
            role: baud_render::FaceRole::Fallback,
            variations: Vec::new(),
        }
    }

    /// The installed face discovery picked, with `role`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn picked(p: &crate::discover::Picked, role: baud_render::FaceRole) -> FaceSpec {
        FaceSpec {
            path: p.path.clone(),
            index: p.index,
            from: match p.origin {
                crate::discover::Origin::File => FaceFrom::File,
                crate::discover::Origin::Embedded(b) => FaceFrom::Embedded(b),
            },
            role,
            variations: p.variations.clone(),
        }
    }

    /// **The face iTerm2's badge is drawn in**, baud's embedded JetBrains
    /// Mono Bold, whatever font the text is in: iTerm2's badge has a face
    /// of its own, `KEY_BADGE_FONT`, apart from the text's (`badgeFont:`,
    /// `[[source: iterm2-964a75d-profile-preferences]]`; Jev,
    /// EMBEDDED_BOLD). Never in the fallback chain (`Stack::badge`).
    fn badge() -> FaceSpec {
        FaceSpec {
            role: baud_render::FaceRole::Badge,
            ..FaceSpec::embedded(crate::embedded::BOLD)
        }
    }

    /// A font compiled into the binary, as the face `style` is drawn from.
    fn embedded_style(e: crate::embedded::Embedded, style: baud_render::FaceStyle) -> FaceSpec {
        FaceSpec {
            role: baud_render::FaceRole::Style(style),
            ..FaceSpec::embedded(e)
        }
    }
}

/// The files a config pulls in: where it is looked up, where its themes
/// are tried, and the fonts it names by path.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Watched {
    pub lookup: Vec<std::path::PathBuf>,
    pub themes: Vec<std::path::PathBuf>,
    pub faces: Vec<std::path::PathBuf>,
}

/// The config at launch ([`Config::launch`]).
pub struct Launched {
    pub config: Config,
    /// The config as read, before its family names are resolved: what a
    /// reload compares its own reading with.
    pub read: Config,
    pub said: Vec<String>,
    pub watched: Watched,
}

/// A config read again that can be applied: its names not yet resolved,
/// what it could not act on, and the files it pulls in.
pub struct Reading {
    pub config: Config,
    pub said: Vec<String>,
    pub watched: Watched,
}

/// What reading the config again found ([`Config::reread`]).
pub enum Reread {
    /// A config to apply.
    Read(Box<Reading>),
    /// A file that does not parse: the sentence the window shows, and the
    /// last good config stays.
    Broken(String),
    /// The file in force is gone, and none above it in the lookup is there.
    Gone(std::path::PathBuf),
}

/// The sentence the window shows while the file in force is gone.
pub fn gone(path: &std::path::Path) -> String {
    format!(
        "{} is gone; the last settings stay until it is back",
        shown(path)
    )
}

/// What a broken line costs: skipped, at launch, or the whole file kept
/// out, on a reload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    Skipped,
    Kept,
}

/// The first line of a file that is not a config line.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Broken {
    /// The line's number, from one.
    line: core::num::NonZeroUsize,
    what: Bad,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Bad {
    /// Not blank, a comment, a `[section]` header or `key = value`; the
    /// line as written.
    NotKeyValue(String),
    /// A `key = value` with nothing before the `=`.
    NoKey,
    /// A list opened on this line and never closed.
    Unclosed,
}

impl Broken {
    fn said(&self, path: &std::path::Path, fault: Fault) -> String {
        let rest = match fault {
            Fault::Skipped => "it is skipped",
            Fault::Kept => "the last good settings stay",
        };
        let what = match &self.what {
            Bad::NotKeyValue(l) => format!("{l:?} is not key = value"),
            Bad::NoKey => "a value with no key".to_string(),
            Bad::Unclosed => "a list opened here is never closed".to_string(),
        };
        format!("{} line {}: {what}; {rest}", shown(path), self.line)
    }
}

/// A path as a sentence names it: under the home folder as `~/`.
fn shown(path: &std::path::Path) -> String {
    let home = absolute_dir(std::env::var_os("HOME"));
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// **The first line [`fields`] cannot read as a config line**, by the
/// rules it reads with: blank and comment lines, a `[section]` header,
/// and `key = value`, whose value may be a list that runs to its closing
/// `]` however many lines that takes. [`fields`] skips such a line
/// without a word, which is what a launch still does with it -- said now.
fn syntax(text: &str) -> Option<Broken> {
    let mut lines = text.lines().enumerate().peekable();
    while let Some((n, line)) = lines.next() {
        let trimmed = strip_comment(line).trim();
        if trimmed.is_empty() || (trimmed.starts_with('[') && trimmed.ends_with(']')) {
            continue;
        }
        let Some((key, value)) = strip_comment(line).split_once('=') else {
            return Some(Broken {
                line: core::num::NonZeroUsize::MIN.saturating_add(n),
                what: Bad::NotKeyValue(line.trim().to_string()),
            });
        };
        if key.trim().is_empty() {
            return Some(Broken {
                line: core::num::NonZeroUsize::MIN.saturating_add(n),
                what: Bad::NoKey,
            });
        }
        let value = value.trim();
        if value.starts_with('[') && !value.contains(']') {
            let closed = lines
                .by_ref()
                .any(|(_, next)| strip_comment(next).contains(']'));
            if !closed {
                return Some(Broken {
                    line: core::num::NonZeroUsize::MIN.saturating_add(n),
                    what: Bad::Unclosed,
                });
            }
        }
    }
    None
}

/// Where the config is looked for, in order, from the environment
/// ([`config_paths`]).
pub fn lookup() -> Vec<std::path::PathBuf> {
    let given = std::env::var_os("BAUD_CONFIG")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p));
    config_paths(
        given,
        absolute_dir(std::env::var_os("XDG_CONFIG_HOME")),
        absolute_dir(std::env::var_os("HOME")),
    )
}

/// The first config file that reads, by the path it was read at.
fn read_first() -> Option<(std::path::PathBuf, String)> {
    lookup()
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
}

/// **Where a config file is looked for**, in order: `given`, from
/// `BAUD_CONFIG`; `$XDG_CONFIG_HOME/baud/config.toml`; and
/// `~/.config/baud/config.toml`. No entry is under the working directory:
/// the `./config.toml` that ended this list was dropped on the owner's
/// call of 2026-10-13, whose option said "No terminal surveyed reads its
/// config from the cwd" (the record, "A shell baud starts from the Dock
/// starts in the home directory"); a Dock launch's is `/`.
///
/// `given` is made absolute by the caller, against the directory baud was
/// started in, which baud never changes: a relative `BAUD_CONFIG` typed
/// in a shell names the file under that shell's directory, and the path
/// the log says, and the `themes` beside it, are that file's.
fn config_paths(
    given: Option<std::path::PathBuf>,
    xdg: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = given.into_iter().collect();
    paths.extend(xdg.map(|x| x.join("baud/config.toml")));
    paths.extend(home.map(|h| h.join(".config/baud/config.toml")));
    paths
}

/// **A directory variable, used only when it names an absolute path.**
/// For `XDG_CONFIG_HOME` the specification says so: "If $XDG_CONFIG_HOME
/// is either not set or empty, a default equal to $HOME/.config should be
/// used", and "If an implementation encounters a relative path in any of
/// these variables it should consider the path invalid and ignore it"
/// (`[[source: xdg-basedir-0-8]]`). `HOME` is held to the same rule, which
/// no text sets for it: an empty or relative one would look for
/// `.config/baud` under the working directory, the lookup this file no
/// longer makes. Both the config's lookup and the themes' read through it.
pub(crate) fn absolute_dir(v: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    v.map(std::path::PathBuf::from).filter(|p| p.is_absolute())
}

/// `(section, key, value)` triples. A value runs to the end of the line, or,
/// when it opens a `[`, to the matching `]` however many lines that takes.
/// Comments (`#`) outside a string and blank lines are skipped.
///
/// **A section header (`[name]` on its own line) opens a namespace**, and
/// the keys after it carry it until the next one; keys before any header
/// carry the empty section. This reader used to drop the header and
/// flatten the keys, which was fine while no two sections wanted the same
/// name and stopped being fine the moment `[glass]` asked for `style`,
/// `hue` and `shadow`.
///
/// A header is told from a list's opening bracket by the line being
/// nothing else: a list is the right-hand side of a `key =`, so it never
/// starts a line.
fn fields(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let line = strip_comment(line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.trim().to_string();
        if value.starts_with('[') && !value.contains(']') {
            // A list across lines: gather until the closing bracket.
            while let Some(next) = lines.peek() {
                let n = strip_comment(next);
                value.push(' ');
                value.push_str(n.trim());
                lines.next();
                if n.contains(']') {
                    break;
                }
            }
        }
        out.push((section.clone(), key.trim().to_string(), value));
    }
    out
}

/// Everything before an unquoted `#`.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The quoted strings in a value, whether a lone `"x"` or a `["a", "b"]` list.
fn strings(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.char_indices();
    while let Some((i, ch)) = chars.next() {
        if ch == '"' {
            let rest = &value[i + 1..];
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_string());
                // Advance past the closing quote.
                for _ in 0..rest[..=end].chars().count() {
                    chars.next();
                }
            }
        }
    }
    out
}

/// The system mono, its colour emoji, its symbols for the braille and box
/// drawing Menlo lacks, and the two faces that carry the CJK scripts --
/// Hiragino Sans GB for Han and kana, Apple SD Gothic Neo for Hangul --
/// which the first three do not: without them a CJK character is a box --
/// and Kohinoor Devanagari, which the corpus carries too. By family name:
/// CSS's search picks each one's regular face, which is the face the paths
/// named before (Menlo.ttc@0 and so on; Hiragino Sans GB's W3, at 300, as
/// 400 searches down before it searches up to the W6).
#[cfg(target_os = "macos")]
const DEFAULT_FACES: [&str; 6] = [
    "Menlo",
    "Apple Color Emoji",
    "Apple Symbols",
    "Hiragino Sans GB",
    "Apple SD Gothic Neo",
    "Kohinoor Devanagari",
];

/// The embedded Regular first -- JetBrains Mono with baud's Hebrew -- the
/// embedded icon face right behind it (Jev's rule, 2026-09-29: it carries
/// only private-use icons, so its place changes nothing for text), then the
/// system faces as they were:
/// Menlo keeps the 1,593 codepoints it maps and JetBrains Mono does not
/// (extended Latin, polytonic Greek, Georgian, more arrows), decided with
/// Jev (0.98), 2026-09-29. Then the embedded STIX Two Math, last in the
/// chain, for the symbols none of those has (the owner's call, 2026-10-02).
/// Last, the three styled faces, which are not in the fallback chain.
#[cfg(target_os = "macos")]
fn default_faces() -> Vec<FaceSpec> {
    [
        FaceSpec::embedded(crate::embedded::REGULAR),
        FaceSpec::embedded(crate::embedded::SYMBOLS),
    ]
    .into_iter()
    .chain(DEFAULT_FACES.iter().map(|s| FaceSpec::parse(s)))
    .chain([FaceSpec::embedded(crate::embedded::STIX)])
    .chain(styled_faces())
    .chain([FaceSpec::badge()])
    .collect()
}

/// The embedded Bold -- JetBrains Mono Bold with baud's Hebrew -- and
/// JetBrains Mono's stock Italic and Bold Italic, each the face its style is
/// drawn from: never synthesised while a real face exists. The italics map
/// no Hebrew for now (STOCK_ITALIC, interim), so italic Hebrew falls to the
/// chain and draws from the embedded Regular.
fn styled_faces() -> [FaceSpec; 3] {
    use baud_render::FaceStyle;
    [
        FaceSpec::embedded_style(crate::embedded::BOLD, FaceStyle::Bold),
        FaceSpec::embedded_style(crate::embedded::ITALIC, FaceStyle::Italic),
        FaceSpec::embedded_style(crate::embedded::BOLD_ITALIC, FaceStyle::BoldItalic),
    ]
}

/// The embedded Regular and icon face first; then, since Linux has no one
/// system mono, DejaVu Sans Mono where it is installed, for what JetBrains
/// Mono does not map, Noto Color Emoji where it is, and the embedded STIX
/// Two Math last, as on macOS. A config names anything else.
#[cfg(not(target_os = "macos"))]
fn default_faces() -> Vec<FaceSpec> {
    [
        FaceSpec::embedded(crate::embedded::REGULAR),
        FaceSpec::embedded(crate::embedded::SYMBOLS),
    ]
    .into_iter()
    .chain(["DejaVu Sans Mono", "Noto Color Emoji"].map(FaceSpec::parse))
    .chain([FaceSpec::embedded(crate::embedded::STIX)])
    .chain(styled_faces())
    .chain([FaceSpec::badge()])
    .collect()
}

/// **Whether a `fonts` value names a file rather than a family**: it holds
/// a path separator, or ends in a font file's extension (`.ttf`, `.otf`,
/// `.ttc`, `.otc`, any case) before any `@N`. Anything else is a family
/// name, as every other terminal's font setting takes one; a path stays
/// accepted, as theirs do, for a font that is not installed.
pub fn names_a_file(entry: &str) -> bool {
    let path = match entry.rsplit_once('@') {
        Some((p, i)) if i.parse::<u32>().is_ok() => p,
        _ => entry,
    };
    let font = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            ["ttf", "otf", "ttc", "otc"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        });
    path.contains('/') || font
}

/// The default stack has to carry what the corpus draws, on the machine
/// it ships on: a test uses a font that supports what it measures, and
/// so does a default. One codepoint each of Han, kana and Hangul, plus a
/// box-drawing glyph and an emoji, must resolve to a glyph in some face
/// of the stack, in stack order.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use baud_domain::Codepoint;
    use baud_font::Face;

    /// Box drawing, block elements and braille are not in the list: baud
    /// draws those itself (`baud_render::boxes`), so no face has to carry
    /// them and the stack is not held to a range it never renders.
    #[test]
    fn the_default_stack_carries_han_kana_hangul_devanagari_and_emoji() {
        let mut c = Config::defaults();
        assert!(c.resolve(crate::discover::Catalogue::system()).is_empty());
        let specs = c.faces;
        let files: Vec<Vec<u8>> = specs
            .iter()
            .map(|f| match f.from {
                FaceFrom::Embedded(b) => b.to_vec(),
                FaceFrom::File => {
                    std::fs::read(&f.path).unwrap_or_else(|e| panic!("{}: {e}", f.path))
                }
                FaceFrom::Family => panic!("{}: not resolved", f.path),
            })
            .collect();
        let faces: Vec<Face<'_>> = files
            .iter()
            .zip(&specs)
            .map(|(d, f)| Face::parse_index(d, f.index).expect("a face"))
            .collect();
        for (name, cp) in [
            ("Han", 0x4E2D),
            ("kana", 0x3042),
            ("Hangul", 0xD55C),
            ("Devanagari", 0x0928),
            ("emoji", 0x1F600),
            ("Claude Code's mode arrow", 0x23F5),
            ("Claude Code's bullet", 0x23FA),
        ] {
            let c = Codepoint::new(cp).expect("a codepoint");
            assert!(
                faces.iter().any(|f| f.glyph(c).is_some()),
                "{name} U+{cp:04X} is in no face of the default stack"
            );
        }
    }
}

/// **Fonts by family name**: what a `fonts` value names, and what resolving
/// the stack against the installed faces makes of it.
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod named {
    use super::*;
    use crate::discover::Catalogue;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

    fn read(text: &str) -> Config {
        let mut c = Config::defaults();
        let said = c.apply(text);
        assert!(said.is_empty(), "{said:?}");
        c
    }

    /// The faces of the four static JetBrains Mono files baud carries, as a
    /// font directory holds them.
    fn jetbrains() -> Catalogue {
        Catalogue::scan(&[format!("{ROOT}/baud-app/fonts/jetbrains-mono").into()])
    }

    /// **A value is a file when it has a path separator or a font file's
    /// extension, and a family name otherwise**, as the other terminals'
    /// font settings take one.
    #[test]
    fn a_fonts_value_is_a_file_or_a_family() {
        for file in [
            "/System/Library/Fonts/Menlo.ttc@0",
            "fonts/Mine.ttf",
            "Mine.TTF",
            "x.otf",
            "y.ttc@2",
            "z.OTC",
        ] {
            assert!(names_a_file(file), "{file}");
        }
        for family in [
            "Menlo",
            "JetBrains Mono",
            "Fira Code@1",
            "Font Awesome 6 Free",
            "a.b",
        ] {
            assert!(!names_a_file(family), "{family}");
        }
        let c = read("fonts = [\"JetBrains Mono\", \"/a.ttf\"]\n");
        assert_eq!(
            (c.faces[0].from, c.faces[0].path.as_str()),
            (FaceFrom::Family, "JetBrains Mono")
        );
        assert_eq!(c.faces[1].from, FaceFrom::File);
        assert_eq!(c.stack, StackFrom::Configured);
        assert_eq!(Config::defaults().stack, StackFrom::Default);
    }

    /// **A family name is not a path beside the config**: the relative
    /// path rule (the owner's call of 2026-10-13) joins a file to the
    /// config's directory, and joining a name there would make `Menlo` a
    /// file that does not exist.
    #[test]
    fn a_family_name_is_not_read_beside_the_config() {
        let mut c = read("fonts = [\"Menlo\", \"fonts/Mine.ttf\"]\n");
        c.faces_beside(std::path::Path::new("/cfg"), None);
        assert_eq!(c.faces[0].path, "Menlo");
        assert_eq!(c.faces[1].path, "/cfg/fonts/Mine.ttf");
    }

    /// **Resolving makes each name the face it picks, says what it could
    /// not find, and keeps the embedded faces behind**: a configured name
    /// that is no installed family and a path that is no file are left out
    /// and said, the stack goes on with the rest.
    #[test]
    fn a_name_resolves_to_its_face_and_a_miss_is_said() {
        let jb = format!("{ROOT}/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
        let mut c =
            read("fonts = [\"No Such Family\", \"jetbrains mono\", \"/no/such/file.ttf\"]\n");
        let said = c.resolve(&jetbrains());
        let c = super::without_badge(c);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("\"No Such Family\""), "{said:?}");
        assert!(said[1].contains("/no/such/file.ttf"), "{said:?}");
        assert_eq!(
            (c.faces[0].path.as_str(), c.faces[0].from),
            (jb.as_str(), FaceFrom::File)
        );
        assert_eq!(c.faces[0].role, baud_render::FaceRole::Fallback);
        assert!(
            c.faces[1..4]
                .iter()
                .all(|f| matches!(f.from, FaceFrom::Embedded(_)))
        );
        assert!(
            c.faces[4..]
                .iter()
                .all(|f| matches!(f.role, baud_render::FaceRole::Style(_)))
        );
        assert_eq!(
            c.faces.len(),
            7,
            "the face, the three embedded fallbacks and its family's three styles"
        );
        assert_eq!(c.stack, StackFrom::Configured);
    }

    /// **baud's embedded font answers to its family's name, as one family
    /// of four, and the icon face to its own** (Jev, E_IN_NAMES and
    /// N_SHARE): with no font installed at all, the embedded Regular is
    /// the first face, its Bold, Italic and Bold Italic the styled faces,
    /// and nothing is said. "JetBrains Mono" is no embedded family: the
    /// stock italics carry the embedded family's name now. An embedded
    /// face wins a tie with an installed copy of itself.
    #[test]
    fn the_embedded_font_answers_to_its_name() {
        use crate::embedded::{BOLD, BOLD_ITALIC, ITALIC, REGULAR, STIX, SYMBOLS};
        use baud_render::{FaceRole, FaceStyle};
        let none = Catalogue::scan(&[]);
        let mut c = read("fonts = [\"proto HEBREW mono c\", \"Symbols Nerd Font Mono\"]\n");
        let said = c.resolve(&none);
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(c.stack, StackFrom::Configured);
        // Which embedded face, told by its bytes: a `const`'s
        // `include_bytes!` may be a copy at each use, merged in a release
        // build and not in a debug one, so an address names no face.
        let bytes = |f: &FaceSpec| match f.from {
            FaceFrom::Embedded(b) => [REGULAR, BOLD, ITALIC, BOLD_ITALIC, SYMBOLS, STIX]
                .into_iter()
                .find(|e| e.bytes == b)
                .map(|e| e.name),
            _ => None,
        };
        assert_eq!(bytes(&c.faces[0]), Some(REGULAR.name));
        assert_eq!(bytes(&c.faces[1]), Some(SYMBOLS.name));
        for (style, e) in [
            (FaceStyle::Bold, BOLD),
            (FaceStyle::Italic, ITALIC),
            (FaceStyle::BoldItalic, BOLD_ITALIC),
        ] {
            let f = c
                .faces
                .iter()
                .find(|f| f.role == FaceRole::Style(style))
                .unwrap_or_else(|| panic!("{style:?}"));
            assert_eq!(bytes(f), Some(e.name), "{style:?}");
        }
        // Ahead of an installed copy: the icon face's own file, scanned as a
        // font directory holds it, loses the tie to the embedded one.
        let installed =
            Catalogue::scan(&[format!("{ROOT}/baud-app/fonts/symbols-nerd-font").into()]);
        let mut icons = read("fonts = [\"Symbols Nerd Font Mono\"]\n");
        assert!(icons.resolve(&installed).is_empty());
        assert_eq!(bytes(&icons.faces[0]), Some(SYMBOLS.name));
        let mut jbm = read("fonts = [\"JetBrains Mono\", \"Symbols Nerd Font Mono\"]\n");
        let said = jbm.resolve(&none);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("\"JetBrains Mono\""), "{said:?}");
        assert!(
            jbm.faces
                .iter()
                .all(|f| !matches!(f.role, FaceRole::Style(_)))
        );
    }

    /// **When nothing a config named is found, the built-in stack is
    /// used, and said**; the built-in stack's own names that are not
    /// installed -- every system name, against a catalogue of JetBrains
    /// Mono alone -- are left out without a word.
    #[test]
    fn nothing_found_is_the_built_in_stack() {
        let mut c = read("fonts = [\"No Such Family\"]\n");
        let said = c.resolve(&jetbrains());
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[1].contains("built-in stack"), "{said:?}");
        assert_eq!(c.stack, StackFrom::Default);
        assert!(
            c.faces
                .iter()
                .all(|f| matches!(f.from, FaceFrom::Embedded(_))),
            "only the embedded faces are left"
        );
        let mut d = Config::defaults();
        assert!(
            d.resolve(&jetbrains()).is_empty(),
            "the defaults' misses are quiet"
        );
        assert!(d.faces.iter().all(|f| f.from != FaceFrom::Family));
    }

    /// Each style's face in `c`, by the file name it was read from and,
    /// for a variable face, its `wght`.
    fn styles(c: &Config) -> Vec<(baud_render::FaceStyle, String, Option<f32>)> {
        let wght = baud_font::Tag::from_bytes(*b"wght");
        c.faces
            .iter()
            .filter_map(|f| match f.role {
                baud_render::FaceRole::Style(s) => Some((
                    s,
                    f.path.rsplit('/').next().unwrap_or("").to_string(),
                    f.variations
                        .iter()
                        .find(|v| v.tag == wght)
                        .map(|v| v.value.to_f32()),
                )),
                baud_render::FaceRole::Fallback | baud_render::FaceRole::Badge => None,
            })
            .collect()
    }

    use baud_render::FaceStyle::{Bold, BoldItalic, Italic};

    /// **A configured family's own Bold, Italic and Bold Italic are its
    /// styled faces** (the owner, 2026-09-29: "if there is bold font, baud
    /// should not fake it and use the bold font"), found by CSS 5.2's
    /// search in the family the first font named, and the stack they make
    /// is one the renderer takes, each style drawn from its own face.
    #[test]
    fn a_configured_family_brings_its_own_styles() {
        let mut c = read("fonts = [\"JetBrains Mono\"]\n");
        assert!(c.resolve(&jetbrains()).is_empty());
        assert_eq!(
            styles(&c),
            [
                (Bold, "JetBrainsMono-Bold.ttf".into(), None),
                (Italic, "JetBrainsMono-Italic.ttf".into(), None),
                (BoldItalic, "JetBrainsMono-BoldItalic.ttf".into(), None),
            ]
        );
        let data: Vec<Vec<u8>> = c
            .faces
            .iter()
            .map(|f| match f.from {
                FaceFrom::Embedded(b) => b.to_vec(),
                _ => std::fs::read(&f.path).unwrap(),
            })
            .collect();
        let faces: Vec<baud_font::Face<'_>> = data
            .iter()
            .zip(&c.faces)
            .map(|(d, f)| baud_font::Face::parse_index(d, f.index).unwrap())
            .collect();
        let roles: Vec<_> = c.faces.iter().map(|f| f.role).collect();
        let stack = baud_render::Stack::with_roles(&faces, &roles).expect("a stack");
        for s in [Bold, Italic, BoldItalic] {
            assert_eq!(stack.effective(s), s);
        }
    }

    /// The four JetBrains Mono files' `names` copied into a directory of
    /// this test's own.
    fn some_of_jetbrains(tag: &str, names: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("baud-b141-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in names {
            std::fs::copy(
                format!("{ROOT}/baud-app/fonts/jetbrains-mono/{n}"),
                dir.join(n),
            )
            .unwrap();
        }
        dir
    }

    /// **A family with a Bold and no italic draws italic from its regular
    /// face and bold italic from its Bold** (CSS 5.2: style first, then
    /// weight; Jev's lean B_CSS); nothing is synthesised (R_REGULAR).
    #[test]
    fn a_family_without_italics_draws_bold_italic_bold() {
        let dir = some_of_jetbrains(
            "noitalic",
            &["JetBrainsMono-Regular.ttf", "JetBrainsMono-Bold.ttf"],
        );
        let mut c = read("fonts = [\"JetBrains Mono\"]\n");
        assert!(
            c.resolve(&Catalogue::scan(std::slice::from_ref(&dir)))
                .is_empty()
        );
        assert_eq!(
            styles(&c),
            [
                (Bold, "JetBrainsMono-Bold.ttf".into(), None),
                (BoldItalic, "JetBrainsMono-Bold.ttf".into(), None),
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A font named by its path brings its family's styles from beside
    /// it and from the installed fonts**: FiraCode-Regular.ttf's directory
    /// holds FiraCode-VF.ttf, whose wght reaches 700, so bold is the
    /// variable face at 700 (Jev, V_AXIS); alone in a directory, with
    /// nothing installed, it is the only face and every style draws from
    /// it.
    #[test]
    fn a_font_named_by_path_brings_its_familys_styles() {
        let path = format!("{ROOT}/baud-font/tests/fonts/FiraCode-Regular.ttf");
        let none = Catalogue::scan(&[]);
        let mut c = read(&format!("fonts = [\"{path}\"]\n"));
        assert!(c.resolve(&none).is_empty());
        assert_eq!(
            styles(&c),
            [
                (Bold, "FiraCode-VF.ttf".into(), Some(700.0)),
                (BoldItalic, "FiraCode-VF.ttf".into(), Some(700.0)),
            ]
        );
        let dir = std::env::temp_dir().join(format!("baud-b141-fira-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let alone = dir.join("FiraCode-Regular.ttf");
        std::fs::copy(&path, &alone).unwrap();
        let mut c = read(&format!("fonts = [\"{}\"]\n", alone.display()));
        assert!(c.resolve(&none).is_empty());
        assert!(styles(&c).is_empty(), "{:?}", styles(&c));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A variable font named by its path is drawn at the weight
    /// searched, as one named by its family is**: FiraCode-VF.ttf, whose
    /// wght runs 300-700 with 300 its default, alone in a directory, is
    /// the regular face at 400 and its own bold at 700, and italic draws
    /// from the regular face as drawn rather than from a second copy of
    /// it.
    #[test]
    fn a_variable_font_named_by_path_is_drawn_at_the_weight_searched() {
        let wght = baud_font::Tag::from_bytes(*b"wght");
        let at = |c: &Config| {
            c.faces[0]
                .variations
                .iter()
                .find(|v| v.tag == wght)
                .map(|v| v.value.to_f32())
        };
        let dir = std::env::temp_dir().join(format!("baud-b141-firavf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vf = dir.join("FiraCode-VF.ttf");
        std::fs::copy(format!("{ROOT}/baud-font/tests/fonts/FiraCode-VF.ttf"), &vf).unwrap();
        let mut by_path = read(&format!("fonts = [\"{}\"]\n", vf.display()));
        assert!(by_path.resolve(&Catalogue::scan(&[])).is_empty());
        let mut by_name = read("fonts = [\"Fira Code\"]\n");
        assert!(
            by_name
                .resolve(&Catalogue::scan(std::slice::from_ref(&dir)))
                .is_empty()
        );
        for c in [&by_path, &by_name] {
            assert_eq!(at(c), Some(400.0));
            assert_eq!(
                styles(c),
                [
                    (Bold, "FiraCode-VF.ttf".into(), Some(700.0)),
                    (BoldItalic, "FiraCode-VF.ttf".into(), Some(700.0)),
                ]
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A name that is a face's PostScript name or full name finds that
    /// face**, as kitty and WezTerm take one (Jev's lean, L_PS_FIRST:
    /// PostScript name, then full name, then family), by CSS's caseless
    /// match, and the face's family brings the other styles: JetBrains
    /// Mono's Bold named either way is the regular face, its Italic and
    /// Bold Italic the styled ones, and no Bold is added beside itself. A
    /// full name equal to the face's family is passed over for the family,
    /// so "JetBrains Mono" still picks the Regular.
    #[test]
    fn a_face_is_found_by_its_postscript_or_full_name() {
        use baud_render::FaceStyle::{BoldItalic, Italic};
        for name in ["JetBrainsMono-Bold", "jetbrains MONO bold"] {
            let mut c = read(&format!("fonts = [\"{name}\"]\n"));
            let said = c.resolve(&jetbrains());
            assert!(said.is_empty(), "{name}: {said:?}");
            assert!(
                c.faces[0].path.ends_with("/JetBrainsMono-Bold.ttf"),
                "{name}"
            );
            assert_eq!(
                styles(&c),
                [
                    (Italic, "JetBrainsMono-Italic.ttf".into(), None),
                    (BoldItalic, "JetBrainsMono-BoldItalic.ttf".into(), None),
                ],
                "{name}"
            );
        }
        let mut c = read("fonts = [\"JetBrains Mono\"]\n");
        assert!(c.resolve(&jetbrains()).is_empty());
        assert!(c.faces[0].path.ends_with("/JetBrainsMono-Regular.ttf"));
        let mut c = read("fonts = [\"JetBrains Mono Bold Italic\", \"JetBrainsMono-Italic\"]\n");
        assert!(c.resolve(&jetbrains()).is_empty());
        assert!(c.faces[0].path.ends_with("/JetBrainsMono-BoldItalic.ttf"));
        assert!(c.faces[1].path.ends_with("/JetBrainsMono-Italic.ttf"));
        // A family and a style run together is no name of the face.
        let mut c = read("fonts = [\"JetBrainsMonoBold\"]\n");
        assert_eq!(c.resolve(&jetbrains()).len(), 2);
    }

    /// JetBrains Mono's file `file` with its `name` table replaced by one
    /// holding `names` (nameID, string) as Windows English (United States)
    /// records, appended and pointed at, as `baud-font/tests/name.rs`
    /// splices one; every other table is the file's own.
    fn renamed(file: &str, names: &[(u16, &str)]) -> Vec<u8> {
        let mut f = std::fs::read(format!("{ROOT}/baud-app/fonts/jetbrains-mono/{file}")).unwrap();
        let n = u16::try_from(names.len()).unwrap();
        let mut table = Vec::new();
        let mut store = Vec::new();
        for v in [0, n, 6 + 12 * n] {
            table.extend_from_slice(&v.to_be_bytes());
        }
        for &(id, s) in names {
            let s: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
            let len = u16::try_from(s.len()).unwrap();
            let off = u16::try_from(store.len()).unwrap();
            for v in [3, 1, 0x409, id, len, off] {
                table.extend_from_slice(&v.to_be_bytes());
            }
            store.extend_from_slice(&s);
        }
        table.extend_from_slice(&store);
        let tables = usize::from(u16::from_be_bytes([f[4], f[5]]));
        let rec = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&r| &f[r..r + 4] == b"name")
            .expect("JetBrains Mono has a name table");
        while !f.len().is_multiple_of(4) {
            f.push(0);
        }
        let at = u32::try_from(f.len()).unwrap();
        f.extend_from_slice(&table);
        f[rec + 8..rec + 12].copy_from_slice(&at.to_be_bytes());
        f[rec + 12..rec + 16].copy_from_slice(&u32::try_from(table.len()).unwrap().to_be_bytes());
        f
    }

    /// **A weight-split family named by its legacy family name keeps the
    /// styles that name answers to**: a static family's Light faces carry
    /// nameID 1 "Split Mono Light" and the full name "Split Mono Light",
    /// and nameID 16 "Split Mono", as Seravek's do. The full name is one
    /// of the face's own family names, so it is passed over and CSS 5.2
    /// picks each style among the faces "Split Mono Light" names: the
    /// Light Italic for italic and bold italic, and no Bold from the wider
    /// family. "Split Mono Light Italic" is a full name alone, and names
    /// its face.
    #[test]
    fn a_family_named_by_its_legacy_name_keeps_its_own_styles() {
        let dir = std::env::temp_dir().join(format!("baud-b145-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (out, from, n1, n2, n4, n6) in [
            (
                "SplitMono-Light.ttf",
                "JetBrainsMono-Regular.ttf",
                "Split Mono Light",
                "Regular",
                "Split Mono Light",
                "SplitMono-Light",
            ),
            (
                "SplitMono-LightItalic.ttf",
                "JetBrainsMono-Italic.ttf",
                "Split Mono Light",
                "Italic",
                "Split Mono Light Italic",
                "SplitMono-LightItalic",
            ),
            (
                "SplitMono-Bold.ttf",
                "JetBrainsMono-Bold.ttf",
                "Split Mono",
                "Bold",
                "Split Mono Bold",
                "SplitMono-Bold",
            ),
            (
                "SplitMono-BoldItalic.ttf",
                "JetBrainsMono-BoldItalic.ttf",
                "Split Mono",
                "Bold Italic",
                "Split Mono Bold Italic",
                "SplitMono-BoldItalic",
            ),
        ] {
            let names = [(1, n1), (2, n2), (4, n4), (6, n6), (16, "Split Mono")];
            std::fs::write(dir.join(out), renamed(from, &names)).unwrap();
        }
        let cat = Catalogue::scan(std::slice::from_ref(&dir));
        let mut c = read("fonts = [\"Split Mono Light\"]\n");
        assert!(c.resolve(&cat).is_empty());
        assert!(c.faces[0].path.ends_with("/SplitMono-Light.ttf"));
        assert_eq!(
            styles(&c),
            [
                (Italic, "SplitMono-LightItalic.ttf".into(), None),
                (BoldItalic, "SplitMono-LightItalic.ttf".into(), None),
            ]
        );
        let mut c = read("fonts = [\"Split Mono Light Italic\"]\n");
        assert!(c.resolve(&cat).is_empty());
        assert!(c.faces[0].path.ends_with("/SplitMono-LightItalic.ttf"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **The system's weight-split families named by a legacy family name**
    /// (macOS only): "Seravek Light" draws italic and bold italic from
    /// Seravek Light Italic and adds no Bold, and "Iowan Old Style Black"
    /// draws both from Iowan Old Style Black Italic, as the family search
    /// picks them. Each face is told by its full name, not its place in
    /// the collection.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_systems_weight_split_families_keep_their_own_styles() {
        let cat = Catalogue::system();
        let full = |f: &FaceSpec| -> String {
            let data = std::fs::read(&f.path).unwrap();
            let d = baud_font::Described::of(&data, f.index).unwrap();
            let mut out = [0u8; 256];
            let name = d
                .full_name()
                .and_then(|n| n.utf8(&mut out).map(<[u8]>::to_vec));
            String::from_utf8(name.unwrap_or_default()).unwrap()
        };
        for (name, styled) in [
            ("Seravek Light", "Seravek Light Italic"),
            ("Iowan Old Style Black", "Iowan Old Style Black Italic"),
        ] {
            let mut c = read(&format!("fonts = [\"{name}\"]\n"));
            assert!(c.resolve(cat).is_empty(), "{name}");
            assert_eq!(full(&c.faces[0]), name);
            let got: Vec<_> = c
                .faces
                .iter()
                .filter_map(|f| match f.role {
                    baud_render::FaceRole::Style(s) => Some((s, full(f))),
                    baud_render::FaceRole::Fallback | baud_render::FaceRole::Badge => None,
                })
                .collect();
            assert_eq!(
                got,
                [
                    (Italic, styled.to_string()),
                    (BoldItalic, styled.to_string())
                ],
                "{name}"
            );
        }
    }

    /// **A later variable font named by path is drawn at the weight
    /// searched, as the first is**: FiraCode-VF.ttf behind JetBrains Mono
    /// draws at `wght` 400, not at its `fvar` default of 300, and brings
    /// no styled faces -- only the first font found does.
    #[test]
    fn a_later_variable_font_named_by_path_is_drawn_at_the_weight_searched() {
        let wght = baud_font::Tag::from_bytes(*b"wght");
        let dir = std::env::temp_dir().join(format!("baud-b145-firavf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vf = dir.join("FiraCode-VF.ttf");
        std::fs::copy(format!("{ROOT}/baud-font/tests/fonts/FiraCode-VF.ttf"), &vf).unwrap();
        let jb = format!("{ROOT}/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
        let mut c = read(&format!("fonts = [\"{jb}\", \"{}\"]\n", vf.display()));
        assert!(c.resolve(&Catalogue::scan(&[])).is_empty());
        assert!(c.faces[1].path.ends_with("FiraCode-VF.ttf"));
        let at: Vec<f32> = c.faces[1]
            .variations
            .iter()
            .filter(|v| v.tag == wght)
            .map(|v| v.value.to_f32())
            .collect();
        assert_eq!(at, [400.0], "{:?}", c.faces[1].variations);
        assert!(c.faces[0].variations.is_empty(), "a static face");
        assert!(
            styles(&c).iter().all(|s| s.1.starts_with("JetBrainsMono-")),
            "{:?}",
            styles(&c)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **Only a path font's own folder is read, not the folders below
    /// it**: a font saved in the home folder must not have the home folder
    /// walked at every launch. JetBrains Mono's Bold one level down is not
    /// found; beside the Regular it is.
    #[test]
    fn a_path_fonts_folder_is_read_one_level() {
        let dir = some_of_jetbrains("onelevel", &["JetBrainsMono-Regular.ttf"]);
        std::fs::create_dir_all(dir.join("below")).unwrap();
        std::fs::copy(
            format!("{ROOT}/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
            dir.join("below/JetBrainsMono-Bold.ttf"),
        )
        .unwrap();
        let regular = dir.join("JetBrainsMono-Regular.ttf");
        let none = Catalogue::scan(&[]);
        let mut c = read(&format!("fonts = [\"{}\"]\n", regular.display()));
        assert!(c.resolve(&none).is_empty());
        assert!(styles(&c).is_empty(), "{:?}", styles(&c));
        std::fs::rename(
            dir.join("below/JetBrainsMono-Bold.ttf"),
            dir.join("JetBrainsMono-Bold.ttf"),
        )
        .unwrap();
        let mut c = read(&format!("fonts = [\"{}\"]\n", regular.display()));
        assert!(c.resolve(&none).is_empty());
        assert_eq!(
            styles(&c).len(),
            2,
            "bold and bold italic: {:?}",
            styles(&c)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **The system's own families, on macOS**: Menlo's four styles are
    /// the four faces of one collection; Hiragino Sans GB has no 700, so
    /// bold is its W6 at 600, as 700 searches up and then down; Apple SD
    /// Gothic Neo's Bold is found by its weight class, 700, though its
    /// `fsSelection` names neither bold nor regular.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_systems_families_bring_their_styles() {
        let cat = Catalogue::system();
        let indexed = |c: &Config| -> Vec<(baud_render::FaceStyle, String)> {
            c.faces
                .iter()
                .filter_map(|f| match f.role {
                    baud_render::FaceRole::Style(s) => {
                        Some((s, format!("{}@{:?}", f.path, f.index)))
                    }
                    baud_render::FaceRole::Fallback | baud_render::FaceRole::Badge => None,
                })
                .collect()
        };
        let at = |p: &str, i| {
            format!(
                "/System/Library/Fonts/{p}@{:?}",
                baud_font::FaceIndex::of(i)
            )
        };
        let mut c = read("fonts = [\"menlo\"]\n");
        assert!(c.resolve(cat).is_empty());
        assert_eq!(
            indexed(&c),
            [
                (Bold, at("Menlo.ttc", 1)),
                (Italic, at("Menlo.ttc", 2)),
                (BoldItalic, at("Menlo.ttc", 3))
            ]
        );
        let mut c = read("fonts = [\"Hiragino Sans GB\"]\n");
        assert!(c.resolve(cat).is_empty());
        assert_eq!(indexed(&c)[0], (Bold, at("Hiragino Sans GB.ttc", 2)));
        let mut c = read("fonts = [\"Apple SD Gothic Neo\"]\n");
        assert!(c.resolve(cat).is_empty());
        assert_eq!(indexed(&c)[0], (Bold, at("AppleSDGothicNeo.ttc", 6)));
    }
}

/// `c` with the badge's face taken out (`FaceSpec::badge`): every stack
/// carries it once, the embedded Bold out of the chain, so a test of the
/// text's stack reads that stack as the text draws from it.
#[cfg(test)]
fn without_badge(mut c: Config) -> Config {
    let at: Vec<usize> = (0..c.faces.len())
        .filter(|&i| c.faces[i].role == baud_render::FaceRole::Badge)
        .collect();
    assert_eq!(at.len(), 1, "one badge face");
    assert!(
        matches!(c.faces[at[0]].from, FaceFrom::Embedded(b) if b == crate::embedded::BOLD.bytes),
        "the embedded Bold"
    );
    c.faces.remove(at[0]);
    c
}

/// The reader, and what sections being a namespace buys.
///
/// These are unit tests rather than a file under `tests/` because
/// `baud-app` is a binary crate and an integration test cannot reach its
/// internals -- the same reason `sys.rs` holds its ABI probes.
#[cfg(test)]
mod reader {
    use super::*;

    fn read(text: &str) -> Config {
        let mut c = Config::defaults();
        c.apply(text);
        c
    }

    /// The defaults: the material and the dims Apple's guidance gives, and
    /// the crate example's own calibration for the rest. Pinned by value:
    /// these are what the record quotes.
    #[test]
    fn the_glass_defaults_are_pinned_by_value() {
        let g = Config::defaults().glass;
        assert_eq!(g.style, Material::Standard);
        assert_eq!(g.style, Material::default(), "the enum agrees");
        assert_eq!(g.shadow, Shadow::Auto);
        assert_eq!((g.dim_dark, g.dim_light), (0.70, 0.60));
        assert_eq!((g.tint_dark, g.tint_light), (0.54, 0.14));
        assert_eq!((g.hue, g.saturation), (0.60, 0.80));
    }

    /// **The default dims keep default text at 4.5:1 over any backdrop**,
    /// the HIG's minimum ([[source: apple-hig-dark-mode]]), at the bound
    /// where the material darkens nothing and the tint is off: the page at
    /// its dim, source-over in encoded sRGB, over flat white and flat black.
    /// Those two are the worst cases for the two halves, and blur cannot
    /// move a flat backdrop. The ratio is WCAG 2.2's, written out here so
    /// the renderer's own arithmetic is not checking itself.
    #[test]
    fn the_default_dims_keep_text_readable_over_white_and_black() {
        use baud_domain::Chan;
        use baud_render::{Argb, Theme};
        fn lum(c: Argb) -> f64 {
            let lin = |c: Chan| {
                let v = f64::from(c.get()) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        }
        let ratio = |a: Argb, b: Argb| {
            let (hi, lo) = (lum(a).max(lum(b)), lum(a).min(lum(b)));
            (hi + 0.05) / (lo + 0.05)
        };
        let g = Glass::defaults();
        let grey = |v: u8| Argb::rgb(Chan::from_u8(v), Chan::from_u8(v), Chan::from_u8(v));
        let mut seen = Vec::new();
        for (half, theme, dim) in [
            ("dark", Theme::dark(), g.dim_dark),
            ("light", Theme::light(), g.dim_light),
        ] {
            for (backdrop, over) in [("white", grey(0xFF)), ("black", grey(0x00))] {
                let page = theme.bg.mix(over, Chan::from_unit(dim));
                seen.push((half, dim, backdrop, ratio(theme.fg, page)));
            }
        }
        let low: Vec<_> = seen.iter().filter(|s| s.3 < 4.5).collect();
        assert!(low.is_empty(), "under 4.5:1: {low:.2?} of {seen:.2?}");
    }

    /// **Reduce Motion turns the default off and leaves a named trail**, the
    /// owner's rule, for every trail: the table the design gave, and the one
    /// the driver acts on through [`TrailPick::effective`].
    #[test]
    fn reduce_motion_turns_the_default_off_and_leaves_a_named_trail() {
        assert_eq!(
            TrailPick::Default.effective(SystemMotion::Reduced),
            CursorTrail::Off,
            "the default under Reduce Motion"
        );
        assert_eq!(
            TrailPick::Default.effective(SystemMotion::Full),
            CursorTrail::Plasma
        );
        for t in TRAILS {
            for m in [SystemMotion::Full, SystemMotion::Reduced] {
                assert_eq!(TrailPick::Named(t).effective(m), t, "{t:?} under {m:?}");
            }
        }
        // Through the reader: nothing said, `true` and a typo all follow the
        // system; a trail by name does not.
        for (text, reduced) in [
            ("", CursorTrail::Off),
            ("[cursor]\ntrail = true\n", CursorTrail::Off),
            ("[cursor]\ntrail = firee\n", CursorTrail::Off),
            ("[cursor]\ntrail = \"fire\"\n", CursorTrail::Fire),
            ("[cursor]\ntrail = \"quad\"\n", CursorTrail::Quad),
        ] {
            assert_eq!(
                read(text).cursor_trail.effective(SystemMotion::Reduced),
                reduced,
                "{text:?} under Reduce Motion"
            );
        }
    }

    /// `trail_scroll` is `ride` unless it says `stay`, quoted or bare, and
    /// anything else is said once and rides.
    #[test]
    fn a_trail_rides_the_scroll_unless_it_stays() {
        assert_eq!(Config::defaults().trail_scroll, TrailScroll::Ride);
        let at = |v: &str| read(&format!("[cursor]\ntrail_scroll = {v}\n")).trail_scroll;
        assert_eq!(at("stay"), TrailScroll::Stay);
        assert_eq!(at("\"stay\""), TrailScroll::Stay);
        assert_eq!(at("\"ride\""), TrailScroll::Ride);
        assert_eq!(at("glide"), TrailScroll::Ride);
        assert_eq!(
            Config::defaults()
                .apply("[cursor]\ntrail_scroll = glide\n")
                .len(),
            1,
            "a typo is said"
        );
        assert_eq!(
            read("trail_scroll = stay\n").trail_scroll,
            TrailScroll::Ride,
            "not at the top level"
        );
    }

    /// `blink_animation`: plasma by default, the three names read, and
    /// anything else said once and the default; Reduce Motion means none
    /// whatever was named.
    #[test]
    fn blink_animation_is_plasma_by_default_and_none_under_reduce_motion() {
        assert_eq!(Config::defaults().blink_animation, BlinkAnimation::Plasma);
        let at = |v: &str| read(&format!("[cursor]\nblink_animation = {v}\n")).blink_animation;
        assert_eq!(at("\"plasma\""), BlinkAnimation::Plasma);
        assert_eq!(at("\"fade\""), BlinkAnimation::Fade);
        assert_eq!(at("\"none\""), BlinkAnimation::None);
        assert_eq!(at("\"fire\""), BlinkAnimation::Fire);
        assert_eq!(at("\"ufo\""), BlinkAnimation::Ufo);
        assert_eq!(at("fade"), BlinkAnimation::Fade, "bare, as other keys read");
        for bad in ["\"saucer\"", "\"embers\"", "true", "\"\""] {
            assert_eq!(at(bad), BlinkAnimation::Plasma, "{bad}");
            let said = Config::defaults().apply(&format!("[cursor]\nblink_animation = {bad}\n"));
            assert_eq!(said.len(), 1, "{bad} is said");
        }
        for a in [
            BlinkAnimation::Plasma,
            BlinkAnimation::Fade,
            BlinkAnimation::Fire,
            BlinkAnimation::Ufo,
            BlinkAnimation::None,
        ] {
            assert_eq!(a.effective(SystemMotion::Reduced), BlinkAnimation::None);
            assert_eq!(a.effective(SystemMotion::Full), a);
        }
    }

    /// `blink_interval_ms`: unset by default, a positive number of
    /// milliseconds read as each phase's length, and anything else said
    /// once and the default.
    #[test]
    fn blink_interval_reads_milliseconds_and_refuses_the_rest() {
        use std::time::Duration;
        assert_eq!(Config::defaults().blink_interval, BlinkInterval::Default);
        let at = |v: &str| read(&format!("[cursor]\nblink_interval_ms = {v}\n")).blink_interval;
        assert_eq!(at("333"), BlinkInterval::Set(Duration::from_millis(333)));
        assert_eq!(at("250"), BlinkInterval::Set(Duration::from_millis(250)));
        assert_eq!(
            at("12.5"),
            BlinkInterval::Set(Duration::from_micros(12_500))
        );
        // Past the ceiling is the ceiling, and not a refusal.
        assert_eq!(at("1e300"), BlinkInterval::Set(crate::caret::CEILING));
        assert_eq!(at("600000"), BlinkInterval::Set(crate::caret::CEILING));
        assert!(
            Config::defaults()
                .apply("[cursor]\nblink_interval_ms = 1e300\n")
                .is_empty()
        );
        for bad in ["0", "-1", "NaN", "inf", "fast"] {
            assert_eq!(at(bad), BlinkInterval::Default, "{bad}");
            let said = Config::defaults().apply(&format!("[cursor]\nblink_interval_ms = {bad}\n"));
            assert_eq!(said.len(), 1, "{bad} is said");
        }
    }

    /// `stop_blinking_after` in seconds: absent and `0` are never, a number
    /// a time, and anything that is not a time is never, said once.
    #[test]
    fn stop_blinking_after_reads_seconds_and_zero_is_never() {
        use std::time::Duration;
        assert_eq!(Config::defaults().cursor_stop, StopAfter::Never);
        let at = |v: &str| read(&format!("[cursor]\nstop_blinking_after = {v}\n")).cursor_stop;
        assert_eq!(at("0"), StopAfter::Never);
        assert_eq!(at("5"), StopAfter::After(Duration::from_secs(5)));
        assert_eq!(at("2.5"), StopAfter::After(Duration::from_millis(2500)));
        for bad in ["-1", "NaN", "inf", "soon"] {
            assert_eq!(at(bad), StopAfter::Never, "{bad}");
            assert_eq!(
                Config::defaults()
                    .apply(&format!("[cursor]\nstop_blinking_after = {bad}\n"))
                    .len(),
                1,
                "{bad} is said"
            );
        }
    }

    /// The cursor trail is the plasma unless `[cursor] trail` names another,
    /// quoted or not -- the owner's call of 2026-09-29, replacing the fire.
    /// `true` and `false` read as they did when the key was a switch -- the
    /// default and none -- and a value that names no trail leaves the
    /// default with a warning rather than being read as one.
    #[test]
    fn the_cursor_trail_is_the_plasma_unless_named() {
        assert_eq!(Config::defaults().cursor_trail, TrailPick::Default);
        assert_eq!(
            TrailPick::Default.effective(SystemMotion::Full),
            CursorTrail::Plasma,
            "the default is the plasma"
        );
        let n = TrailPick::Named;
        for (value, want) in [
            ("fire", n(CursorTrail::Fire)),
            ("\"fire\"", n(CursorTrail::Fire)),
            ("plasma", n(CursorTrail::Plasma)),
            ("\"plasma\"", n(CursorTrail::Plasma)),
            ("lightning", n(CursorTrail::Lightning)),
            ("\"lightning\"", n(CursorTrail::Lightning)),
            ("ice", n(CursorTrail::Ice)),
            ("\"ice\"", n(CursorTrail::Ice)),
            ("ufo", n(CursorTrail::Ufo)),
            ("\"ufo\"", n(CursorTrail::Ufo)),
            ("orb", n(CursorTrail::Orb)),
            ("\"orb\"", n(CursorTrail::Orb)),
            ("liquid", n(CursorTrail::Liquid)),
            ("\"liquid\"", n(CursorTrail::Liquid)),
            ("quad", n(CursorTrail::Quad)),
            ("\"quad\"", n(CursorTrail::Quad)),
            ("ghost", n(CursorTrail::Ghost)),
            ("\"ghost\"", n(CursorTrail::Ghost)),
            ("sweep", n(CursorTrail::Sweep)),
            ("\"sweep\"", n(CursorTrail::Sweep)),
            ("off", n(CursorTrail::Off)),
            ("\"off\"", n(CursorTrail::Off)),
            // The default's word, and a value that names no trail, are the
            // default: neither names a trail Reduce Motion should keep.
            ("true", TrailPick::Default),
            ("false", n(CursorTrail::Off)),
            ("yes", TrailPick::Default),
        ] {
            assert_eq!(
                read(&format!("[cursor]\ntrail = {value}\n")).cursor_trail,
                want,
                "trail = {value}"
            );
        }
        assert_eq!(
            Config::defaults().apply("[cursor]\ntrail = yes\n").len(),
            1,
            "an unreadable value is said"
        );
        use crate::trail::Style;
        use baud_render::Shape;
        use baud_render::effect::Effect;
        assert_eq!(CursorTrail::Fire.style(), Style::Effect(Effect::Fire));
        assert_eq!(CursorTrail::Plasma.style(), Style::Effect(Effect::Plasma));
        assert_eq!(
            CursorTrail::Lightning.style(),
            Style::Effect(Effect::Lightning)
        );
        assert_eq!(CursorTrail::Ice.style(), Style::Effect(Effect::Ice));
        assert_eq!(CursorTrail::Ufo.style(), Style::Effect(Effect::Ufo));
        assert_eq!(CursorTrail::Orb.style(), Style::Effect(Effect::Orb));
        assert_eq!(CursorTrail::Liquid.style(), Style::Effect(Effect::Liquid));
        assert_eq!(CursorTrail::Sweep.style(), Style::Smear(Shape::Sweep));
        assert_eq!(CursorTrail::Quad.style(), Style::Smear(Shape::Quad));
    }

    /// Every trail, and the name `[cursor] trail` reads it by. The `match`
    /// is exhaustive, so a new trail does not compile until it is named
    /// here, beside the list it has to join.
    const TRAILS: [CursorTrail; 11] = [
        CursorTrail::Off,
        CursorTrail::Ghost,
        CursorTrail::Sweep,
        CursorTrail::Quad,
        CursorTrail::Fire,
        CursorTrail::Plasma,
        CursorTrail::Lightning,
        CursorTrail::Ice,
        CursorTrail::Ufo,
        CursorTrail::Orb,
        CursorTrail::Liquid,
    ];
    fn trail_name(t: CursorTrail) -> &'static str {
        match t {
            CursorTrail::Off => "off",
            CursorTrail::Ghost => "ghost",
            CursorTrail::Sweep => "sweep",
            CursorTrail::Quad => "quad",
            CursorTrail::Fire => "fire",
            CursorTrail::Plasma => "plasma",
            CursorTrail::Lightning => "lightning",
            CursorTrail::Ice => "ice",
            CursorTrail::Ufo => "ufo",
            CursorTrail::Orb => "orb",
            CursorTrail::Liquid => "liquid",
        }
    }

    /// **docs/cursor.md shows every trail the reader knows, and nothing
    /// else.** Each trail but `off` has its `trail = "<name>"` and its still
    /// in the page, a clip and a still in `docs/cursor/`, and both named by
    /// docs/cursor.html; the default is the one both pages mark as it. So a
    /// new trail fails here until scripts/trail-clips.sh has recorded it and
    /// both pages show it, and a clip or a page entry for a trail that is
    /// gone fails too. The files themselves are checked by name and size,
    /// not by content; the recording script checks the content.
    #[test]
    fn the_cursor_page_shows_every_trail() {
        let docs = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs"));
        let md = std::fs::read_to_string(docs.join("cursor.md")).expect("docs/cursor.md");
        let html = std::fs::read_to_string(docs.join("cursor.html")).expect("docs/cursor.html");
        // `TRAILS` is a list kept by hand, so it is held to the names the
        // reader lists when it refuses one: a trail the reader learns and the
        // list misses fails here rather than passing without a clip.
        let said = Config::defaults().apply("[cursor]\ntrail = bogus\n");
        let mut listed: Vec<&str> = said
            .first()
            .and_then(|s| s.split_once("is not "))
            .and_then(|(_, rest)| rest.split_once(';'))
            .map(|(names, _)| {
                names
                    .split([',', ' '])
                    .filter(|w| !w.is_empty() && *w != "or")
                    .collect()
            })
            .unwrap_or_default();
        let mut named: Vec<&str> = TRAILS.iter().map(|&t| trail_name(t)).collect();
        listed.sort_unstable();
        named.sort_unstable();
        assert_eq!(named, listed, "TRAILS and the reader's own list disagree");
        let mut shown = Vec::new();
        for t in TRAILS {
            let name = trail_name(t);
            assert_eq!(
                read(&format!("[cursor]\ntrail = \"{name}\"\n")).cursor_trail,
                TrailPick::Named(t),
                "the reader does not read {name:?} as {t:?}"
            );
            if t == CursorTrail::Off {
                continue;
            }
            shown.push(name);
            assert!(
                md.contains(&format!("`trail = \"{name}\"`")),
                "cursor.md: no trail = {name:?}"
            );
            assert!(
                md.contains(&format!("](cursor/{name}.png)")),
                "cursor.md: no still of {name}"
            );
            assert!(
                html.contains(&format!("src=\"cursor/{name}.mp4\"")),
                "cursor.html: no clip of {name}"
            );
            assert!(
                html.contains(&format!("poster=\"cursor/{name}.png\"")),
                "cursor.html: no still of {name}"
            );
            for (ext, floor) in [("mp4", 10_000), ("png", 5_000)] {
                let len = std::fs::metadata(docs.join(format!("cursor/{name}.{ext}")))
                    .map_or(0, |m| m.len());
                assert!(len >= floor, "docs/cursor/{name}.{ext} is {len} bytes");
            }
        }
        // Nothing in the directory or on the page names a trail that is gone.
        let mut files = 0;
        for e in std::fs::read_dir(docs.join("cursor")).expect("docs/cursor/") {
            let file = e
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a name");
            let known = file
                .rsplit_once('.')
                .is_some_and(|(n, ext)| shown.contains(&n) && matches!(ext, "mp4" | "png"));
            assert!(known, "docs/cursor/{file} is no trail's clip or still");
            files += 1;
        }
        for clip in html.split("cursor/").skip(1) {
            let name = clip.split(['.', '"']).next().unwrap_or("");
            assert!(
                shown.contains(&name),
                "cursor.html names cursor/{name}, no trail's"
            );
        }
        // The default is the one both pages mark. The page's article is found
        // by its class: the stylesheet names `is-default` first, and a search
        // for the bare word read the first article whatever was marked.
        let default = trail_name(CursorTrail::default());
        assert!(
            md.contains(&format!("## {default}, the default")),
            "cursor.md: {default} is not marked the default"
        );
        let marked = html
            .split_once("class=\"trail is-default\"")
            .and_then(|(_, rest)| rest.split_once("trail = \""))
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(name, _)| name);
        assert_eq!(
            marked,
            Some(default),
            "cursor.html marks another trail the default"
        );
        // The controls: the scan read the pages and the directory at all.
        assert_eq!(shown.len(), TRAILS.len() - 1);
        assert_eq!(files, 2 * shown.len());
    }

    /// The cursor is a blinking block unless the config names another shape
    /// or turns the blink off, and either key alone keeps the other axis.
    /// A value the reader does not know keeps the default and says so.
    #[test]
    fn the_cursor_is_a_blinking_block_unless_configured() {
        let d = Config::defaults();
        assert_eq!(
            (d.cursor_shape, d.cursor_blink),
            (CursorShape::Block, BlinkChoice::Auto)
        );
        assert_eq!(d.cursor_blink.power_on(), CursorBlink::Blinking);
        let c = read("[cursor]\nshape = \"bar\"\nblink = false\n");
        assert_eq!(
            (c.cursor_shape, c.cursor_blink),
            (CursorShape::Bar, BlinkChoice::Off)
        );
        assert_eq!(c.cursor_blink.power_on(), CursorBlink::Steady);
        // Either order, and unquoted.
        let c = read("[cursor]\nblink = false\nshape = underline\n");
        assert_eq!(
            (c.cursor_shape, c.cursor_blink),
            (CursorShape::Underline, BlinkChoice::Off)
        );
        let c = read("[cursor]\nshape = \"bar\"\n");
        assert_eq!(
            (c.cursor_shape, c.cursor_blink),
            (CursorShape::Bar, BlinkChoice::Auto)
        );
        // `true` is an explicit blink, and `auto`, quoted or not, the
        // system's.
        assert_eq!(
            read("[cursor]\nblink = true\n").cursor_blink,
            BlinkChoice::On
        );
        assert_eq!(
            read("[cursor]\nblink = auto\n").cursor_blink,
            BlinkChoice::Auto
        );
        assert_eq!(
            read("[cursor]\nblink = \"auto\"\n").cursor_blink,
            BlinkChoice::Auto
        );
        let c = read("[cursor]\nshape = \"box\"\nblink = maybe\n");
        assert_eq!(
            (c.cursor_shape, c.cursor_blink),
            (CursorShape::Block, BlinkChoice::Auto)
        );
        assert_eq!(
            Config::defaults()
                .apply("[cursor]\nshape = \"box\"\nblink = maybe\n")
                .len(),
            2
        );
        // Not at the top level: `shape` there is nobody's key.
        assert_eq!(read("shape = \"bar\"\n").cursor_shape, CursorShape::Block);
    }

    /// The screen transition is off unless `[screen] transition = true`, and
    /// a value that is neither leaves it off with a warning.
    #[test]
    fn the_screen_transition_is_off_unless_asked_for() {
        assert_eq!(Config::defaults().screen_transition, ScreenTransition::Off);
        assert_eq!(
            read("[screen]\ntransition = true\n").screen_transition,
            ScreenTransition::On
        );
        assert_eq!(
            read("[screen]\ntransition = false\n").screen_transition,
            ScreenTransition::Off
        );
        assert_eq!(
            read("[screen]\ntransition = maybe\n").screen_transition,
            ScreenTransition::Off
        );
    }

    /// **A program may log unless `[screen] logging = "off"`** (the owner's
    /// call of 2026-10-20), quoted or bare; any other value keeps the
    /// default and says so, and the key is the `[screen]` section's.
    #[test]
    fn logging_is_on_unless_turned_off() {
        use baud_domain::Logging;
        assert_eq!(Config::defaults().logging, Logging::On);
        assert_eq!(read("").logging, Logging::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[screen]\nlogging = {off}\n");
            assert_eq!(read(&text).logging, Logging::Off, "{off}");
        }
        assert_eq!(read("[screen]\nlogging = \"on\"\n").logging, Logging::On);
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nlogging = \"yes\"\n");
        assert_eq!(c.logging, Logging::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("logging"), "{said:?}");
        assert_eq!(
            read("logging = \"off\"\n").logging,
            Logging::On,
            "not a top-level key"
        );
    }

    /// **A printed picture's four choices are off unless turned on**: each
    /// `"on"` takes its other value, and anything but on or off keeps off
    /// and says so.
    #[test]
    fn the_graphics_print_choices_are_off_unless_turned_on() {
        use baud_domain::{
            GraphicsPrint, PrintBackground, PrintColour, PrintRotation, PrintSyntax,
        };
        assert_eq!(read("").graphics_print, GraphicsPrint::DEFAULT);
        let all = read(
            "[screen]\nprint_colour = \"on\"\nprint_rgb = \"on\"\nprint_background = \"on\"\nprint_rotated = \"on\"\n",
        );
        assert_eq!(
            all.graphics_print,
            GraphicsPrint {
                colour: PrintColour::Colour,
                syntax: PrintSyntax::Rgb,
                background: PrintBackground::On,
                rotation: PrintRotation::Rotated,
            }
        );
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nprint_colour = \"yes\"\n");
        assert_eq!(c.graphics_print, GraphicsPrint::DEFAULT);
        assert!(said[0].contains("print_colour"), "{said:?}");
    }

    /// **A program's notification is delivered unless `[screen]
    /// notifications = "off"`**, and a value that is neither is said and
    /// read as on.
    #[test]
    fn notifications_are_on_unless_turned_off() {
        assert_eq!(Config::defaults().notifications, Notifications::On);
        assert_eq!(read("").notifications, Notifications::On);
        assert_eq!(
            read("[screen]\nnotifications = \"off\"\n").notifications,
            Notifications::Off
        );
        assert_eq!(
            read("[screen]\nnotifications = off\n").notifications,
            Notifications::Off
        );
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nnotifications = \"answer\"\n");
        assert_eq!(c.notifications, Notifications::On);
        assert!(said[0].contains("notifications"), "{said:?}");
    }

    /// **A program's print opens the panel unless `[screen] printing =
    /// "off"`**, quoted or bare; any other value keeps the default and says
    /// so, and the key is the `[screen]` section's.
    #[test]
    fn printing_is_on_unless_turned_off() {
        assert_eq!(Config::defaults().printing, Printing::On);
        assert_eq!(read("").printing, Printing::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[screen]\nprinting = {off}\n");
            assert_eq!(read(&text).printing, Printing::Off, "{off}");
        }
        assert_eq!(read("[screen]\nprinting = \"on\"\n").printing, Printing::On);
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nprinting = \"yes\"\n");
        assert_eq!(c.printing, Printing::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("printing"), "{said:?}");
        assert_eq!(
            read("printing = \"off\"\n").printing,
            Printing::On,
            "not a top-level key"
        );
    }

    /// **The window operations are performed unless `[window] operations =
    /// "off"`** (the owner's call of 2026-10-19, option 4), quoted or bare;
    /// any other value keeps the default and says so, and the key is the
    /// `[window]` section's, not the top level's.
    #[test]
    fn the_window_operations_are_on_unless_turned_off() {
        use baud_domain::WindowOps;
        assert_eq!(Config::defaults().window_ops, WindowOps::On);
        assert_eq!(read("").window_ops, WindowOps::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[window]\noperations = {off}\n");
            assert_eq!(read(&text).window_ops, WindowOps::Off, "{off}");
        }
        assert_eq!(
            read("[window]\noperations = \"on\"\n").window_ops,
            WindowOps::On
        );
        let mut c = Config::defaults();
        let said = c.apply("[window]\noperations = \"sometimes\"\n");
        assert_eq!(c.window_ops, WindowOps::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("operations"), "{said:?}");
        assert_eq!(
            read("operations = \"off\"\n").window_ops,
            WindowOps::On,
            "a top-level key is not the [window] one"
        );
    }

    /// **A program may change the font unless `[font] changes = "off"`**,
    /// quoted or bare; any other value keeps the default and says so.
    #[test]
    fn a_program_may_change_the_font_unless_turned_off() {
        use baud_domain::FontChanges;
        assert_eq!(Config::defaults().font_changes, FontChanges::On);
        assert_eq!(read("").font_changes, FontChanges::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[font]\nchanges = {off}\n");
            assert_eq!(read(&text).font_changes, FontChanges::Off, "{off}");
        }
        let mut c = Config::defaults();
        let said = c.apply("[font]\nchanges = \"query\"\n");
        assert_eq!(c.font_changes, FontChanges::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("changes"), "{said:?}");
        assert_eq!(
            read("changes = \"off\"\n").font_changes,
            FontChanges::On,
            "a top-level key is not the [font] one"
        );
    }

    /// **A key hides the pointer unless `[pointer] hide_while_typing =
    /// "off"`**, quoted or bare; any other value keeps the default and says
    /// so, and the key is the `[pointer]` section's.
    #[test]
    fn the_pointer_hides_while_typing_unless_turned_off() {
        use baud_domain::PointerMode;
        assert_eq!(Config::defaults().pointer_mode, PointerMode::UnlessTracking);
        assert_eq!(read("").pointer_mode, PointerMode::UnlessTracking);
        for off in ["\"off\"", "off"] {
            let text = format!("[pointer]\nhide_while_typing = {off}\n");
            assert_eq!(read(&text).pointer_mode, PointerMode::Never, "{off}");
        }
        assert_eq!(
            read("[pointer]\nhide_while_typing = \"on\"\n").pointer_mode,
            PointerMode::UnlessTracking
        );
        let mut c = Config::defaults();
        let said = c.apply("[pointer]\nhide_while_typing = \"2\"\n");
        assert_eq!(c.pointer_mode, PointerMode::UnlessTracking);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("hide_while_typing"), "{said:?}");
        assert_eq!(
            read("hide_while_typing = \"off\"\n").pointer_mode,
            PointerMode::UnlessTracking,
            "a top-level key is not the [pointer] one"
        );
    }

    /// **A program's badge is drawn unless `[screen] badge = "off"`**, quoted
    /// or bare; any other value keeps it on and says so.
    #[test]
    fn the_badge_is_on_unless_turned_off() {
        assert_eq!(Config::defaults().badge, BadgeShown::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[screen]\nbadge = {off}\n");
            assert_eq!(read(&text).badge, BadgeShown::Off, "{off}");
        }
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nbadge = \"no\"\n");
        assert_eq!(c.badge, BadgeShown::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("badge"), "{said:?}");
    }

    /// **The cursor guide is off unless `[cursor] guide = "on"`**, quoted
    /// or bare; any other value keeps it off and says so.
    #[test]
    fn the_cursor_guide_is_off_unless_turned_on() {
        use baud_domain::CursorGuide;
        assert_eq!(Config::defaults().cursor_guide, CursorGuide::Hidden);
        for on in ["\"on\"", "on"] {
            let text = format!("[cursor]\nguide = {on}\n");
            assert_eq!(read(&text).cursor_guide, CursorGuide::Shown, "{on}");
        }
        assert_eq!(
            read("[cursor]\nguide = \"off\"\n").cursor_guide,
            CursorGuide::Hidden
        );
        let mut c = Config::defaults();
        let said = c.apply("[cursor]\nguide = \"yes\"\n");
        assert_eq!(c.cursor_guide, CursorGuide::Hidden);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("guide"), "{said:?}");
    }

    /// **Exit-status marks are off unless `[screen] exit_status = "on"`**,
    /// quoted or bare; any other value keeps them off and says so, and the
    /// key is the `[screen]` section's.
    #[test]
    fn exit_status_marks_are_off_unless_turned_on() {
        use baud_render::StatusMarks;
        assert_eq!(Config::defaults().exit_status, StatusMarks::Off);
        assert_eq!(read("").exit_status, StatusMarks::Off);
        for on in ["\"on\"", "on"] {
            let text = format!("[screen]\nexit_status = {on}\n");
            assert_eq!(read(&text).exit_status, StatusMarks::On, "{on}");
        }
        assert_eq!(
            read("[screen]\nexit_status = \"off\"\n").exit_status,
            StatusMarks::Off
        );
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nexit_status = \"yes\"\n");
        assert_eq!(c.exit_status, StatusMarks::Off);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("exit_status"), "{said:?}");
        assert_eq!(
            read("exit_status = \"on\"\n").exit_status,
            StatusMarks::Off,
            "not a top-level key"
        );
    }

    /// **The screen checksum is answered unless `[screen] checksum = "off"`**
    /// (the owner's call of 2026-10-19, option 3), quoted or bare; any other
    /// value keeps the default and says so, and the key is the `[screen]`
    /// section's, not the top level's.
    #[test]
    fn the_screen_checksum_is_answered_unless_turned_off() {
        use baud_domain::ChecksumReport;
        assert_eq!(Config::defaults().checksum, ChecksumReport::On);
        assert_eq!(read("").checksum, ChecksumReport::On);
        for off in ["\"off\"", "off"] {
            let text = format!("[screen]\nchecksum = {off}\n");
            assert_eq!(read(&text).checksum, ChecksumReport::Off, "{off}");
        }
        assert_eq!(
            read("[screen]\nchecksum = \"on\"\n").checksum,
            ChecksumReport::On
        );
        let mut c = Config::defaults();
        let said = c.apply("[screen]\nchecksum = \"maybe\"\n");
        assert_eq!(c.checksum, ChecksumReport::On);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("checksum"), "{said:?}");
        assert_eq!(
            read("checksum = \"off\"\n").checksum,
            ChecksumReport::On,
            "not a top-level key"
        );
    }

    /// Scroll is smooth unless `[scroll] mode = normal`, and a value that is
    /// neither `normal` nor `smooth` falls back to the default, smooth, with
    /// a warning. The quoted form is the one `config.toml.example` writes,
    /// and it is the one the first version of the reader missed.
    #[test]
    fn the_scroll_mode_is_smooth_unless_asked_for() {
        assert_eq!(Config::defaults().scroll, ScrollMode::Smooth);
        assert_eq!(
            read("[scroll]\nmode = \"normal\"\n").scroll,
            ScrollMode::Normal
        );
        assert_eq!(read("[scroll]\nmode = normal\n").scroll, ScrollMode::Normal);
        assert_eq!(read("[scroll]\nmode = smooth\n").scroll, ScrollMode::Smooth);
        assert_eq!(read("[scroll]\nmode = fast\n").scroll, ScrollMode::Smooth);
    }

    #[test]
    fn advertising_sixel_is_off_unless_asked() {
        use baud_domain::SixelClaim;
        assert_eq!(Config::defaults().sixel, SixelClaim::No);
        assert_eq!(
            read("[images]\nadvertise_sixel = true\n").sixel,
            SixelClaim::Yes
        );
        assert_eq!(
            read("[images]\nadvertise_sixel = false\n").sixel,
            SixelClaim::No
        );
        assert_eq!(
            read("[images]\nadvertise_sixel = maybe\n").sixel,
            SixelClaim::No
        );
        // A top-level key of the same name is not the images section's.
        assert_eq!(read("advertise_sixel = true\n").sixel, SixelClaim::No);
    }

    /// **`[clipboard] copy_on_select` is on unless it says off**, the
    /// owner's V13: `on` and `off` read, and anything else keeps on and
    /// says so.
    #[test]
    fn copy_on_select_is_on_unless_it_says_off() {
        assert_eq!(
            Config::defaults().clipboard.copy_on_select,
            CopyOnSelect::On
        );
        for (v, want) in [
            ("\"on\"", CopyOnSelect::On),
            ("\"off\"", CopyOnSelect::Off),
            ("off", CopyOnSelect::Off),
        ] {
            let text = format!("[clipboard]\ncopy_on_select = {v}\n");
            assert_eq!(read(&text).clipboard.copy_on_select, want, "{v}");
        }
        let mut c = Config::defaults();
        let said = c.apply("[clipboard]\ncopy_on_select = \"yes\"\n");
        assert_eq!(c.clipboard.copy_on_select, CopyOnSelect::On);
        assert!(
            said.iter()
                .any(|s| s.contains("copy_on_select") && s.contains("on or off")),
            "{said:?}"
        );
    }

    /// `[clipboard]`: writes on by default with a 256 KiB body, `off` turns
    /// them off, `body_kb` sizes the copy, and a value that does not parse
    /// keeps the default and says so.
    #[test]
    fn a_clipboard_section_sets_the_policy_and_the_size() {
        let d = Config::defaults().clipboard;
        assert_eq!(d.osc52, Osc52::Write, "on by default: the owner's V11");
        assert_eq!(
            d.budget.text(),
            192 << 10,
            "a 256 KiB body is 192 KiB of text"
        );
        assert_eq!(
            read("[clipboard]\nosc52 = \"off\"\n").clipboard.osc52,
            Osc52::Off
        );
        assert_eq!(
            read("[clipboard]\nosc52 = \"write\"\n").clipboard.osc52,
            Osc52::Write
        );
        assert_eq!(
            read("[clipboard]\nbody_kb = 4\n").clipboard.budget.text(),
            3 << 10
        );
        let mut c = Config::defaults();
        let said = c.apply("[clipboard]\nosc52 = \"maybe\"\nbody_kb = lots\nfocus = 1\n");
        assert_eq!(c.clipboard, Clipboard::DEFAULT);
        assert_eq!(said.len(), 3, "{said:?}");
        // A top-level key of the same name is not the section's.
        assert_eq!(read("osc52 = \"off\"\n").clipboard.osc52, Osc52::Write);
    }

    /// `[images]` sets the arena in the reader's unit: `pixels_mb` MiB of
    /// ARGB become `mb << 18` ColorWords. A non-default value, so a section
    /// that lands as "the defaults anyway" cannot pass this. A whole number
    /// that is not one keeps the default rather than being read as zero,
    /// which a zero-size arena would refuse every image silently. `body_kb`,
    /// which sized the raw-Sixel scratch until 2026-10-14, changes nothing
    /// and is said by name, not as an unknown key.
    #[test]
    fn an_images_section_sets_the_arena_and_says_body_kb_is_gone() {
        assert_eq!(Config::defaults().images, crate::host::IMAGE_BUDGET);
        let c = read("[images]\npixels_mb = 8\n");
        assert_eq!(c.images.pixels, 8 << 18, "8 MiB of ColorWords");
        let c = read("[images]\npixels_mb = plenty\n");
        assert_eq!(c.images.pixels, crate::host::IMAGE_BUDGET.pixels);
        let mut c = Config::defaults();
        let said = c.apply("[images]\nbody_kb = 512\npixels_mb = 8\n");
        assert_eq!(c.images.pixels, 8 << 18, "the arena still read");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].contains("body_kb no longer limits") && !said[0].contains("has no key"),
            "{said:?}"
        );
    }

    #[test]
    fn a_glass_section_sets_every_one_of_its_keys() {
        let c = read(
            r#"
[glass]
style      = "clear"
dim_dark   = 0.3
dim_light  = 0.05
tint_dark  = 0.4
tint_light = 0.2
hue        = 0.1
saturation = 0.9
shadow     = "never"
"#,
        );
        assert_eq!(c.glass.style, Material::Clear);
        assert_eq!(c.glass.shadow, Shadow::Never);
        assert_eq!((c.glass.dim_dark, c.glass.dim_light), (0.3, 0.05));
        assert_eq!((c.glass.tint_dark, c.glass.tint_light), (0.4, 0.2));
        assert_eq!((c.glass.hue, c.glass.saturation), (0.1, 0.9));
    }

    /// **The half that fails on the reader this replaced.** It flattened
    /// the headers, so a top-level `style` and `[glass] style` were one
    /// key -- and `[glass]`'s names are generic enough that the collision
    /// is not hypothetical.
    #[test]
    fn a_top_level_key_does_not_reach_the_glass_section() {
        let c = read("style = \"clear\"\nhue = 0.1\nshadow = \"never\"\n");
        assert_eq!(c.glass.style, Material::Standard, "top-level style leaked");
        assert_eq!(c.glass.hue, 0.60, "top-level hue leaked");
        assert_eq!(c.glass.shadow, Shadow::Auto, "top-level shadow leaked");
    }

    /// And the other half: a section must not swallow the keys that are
    /// not in one, whether it comes before them or after.
    #[test]
    fn top_level_keys_parse_around_a_section() {
        let c = read("font_size = 20\n[glass]\ndim_dark = 0.3\n");
        assert_eq!(c.font_size, 20.0);
        assert_eq!(c.glass.dim_dark, 0.3);
        // A key under `[glass]` that belongs to no section is not a
        // top-level key either: `font_size` below is the glass section's.
        let c = read("[glass]\ndim_dark = 0.3\nfont_size = 20\n");
        assert_eq!(c.font_size, 13.0, "a sectioned font_size set the top level");
        assert_eq!(c.glass.dim_dark, 0.3);
    }

    /// `[glass] app_backgrounds` defaults to dimming every background an
    /// application sets, takes `opaque`, falls back to `dim` on a word it
    /// does not know, and is not reached from the top level. Its first name,
    /// `images`, still sets it.
    #[test]
    fn app_backgrounds_are_dimmed_unless_asked_for_opaque() {
        use baud_render::AppBg;
        let get = |t: &str| read(t).glass.app_backgrounds;
        assert_eq!(get(""), AppBg::Dimmed);
        assert_eq!(
            get("[glass]\napp_backgrounds = \"opaque\"\n"),
            AppBg::Opaque
        );
        assert_eq!(get("[glass]\napp_backgrounds = \"dim\"\n"), AppBg::Dimmed);
        assert_eq!(get("[glass]\napp_backgrounds = \"clear\"\n"), AppBg::Dimmed);
        assert_eq!(get("app_backgrounds = \"opaque\"\n"), AppBg::Dimmed);
        assert_eq!(get("[glass]\nimages = \"opaque\"\n"), AppBg::Opaque);
        assert_eq!(get("images = \"opaque\"\n"), AppBg::Dimmed);
    }

    /// `[glass] pictures` defaults to drawing a text-drawn picture's
    /// backgrounds solid, takes `dim`, falls back to `opaque` on a word it
    /// does not know, and is not reached from the top level.
    #[test]
    fn pictures_are_opaque_unless_asked_for_dim() {
        use baud_render::PictureBg;
        let get = |t: &str| read(t).glass.pictures;
        assert_eq!(get(""), PictureBg::Opaque);
        assert_eq!(get("[glass]\npictures = \"dim\"\n"), PictureBg::Dimmed);
        assert_eq!(get("[glass]\npictures = \"opaque\"\n"), PictureBg::Opaque);
        assert_eq!(get("[glass]\npictures = \"solid\"\n"), PictureBg::Opaque);
        assert_eq!(get("pictures = \"dim\"\n"), PictureBg::Opaque);
    }

    /// `[palette] floor`: a trigger from 1 to 21 or "off"; the target is the
    /// larger of 4.5 and the trigger; anything else keeps the default and
    /// says so; and the key belongs to `[palette]`, not the top level.
    #[test]
    fn the_floor_is_a_trigger_or_off() {
        use baud_render::Floor;
        let get = |t: &str| {
            let mut c = Config::defaults();
            let said = c.apply(t);
            (c.floor, said.len())
        };
        assert_eq!(get(""), (Floor::DEFAULT, 0));
        assert_eq!(get("[palette]\nfloor = \"off\"\n"), (Floor::Off, 0));
        assert_eq!(get("[palette]\nfloor = off\n"), (Floor::Off, 0));
        assert_eq!(
            get("[palette]\nfloor = 3\n"),
            (
                Floor::Lift {
                    trigger: 3.0,
                    target: 4.5
                },
                0
            )
        );
        assert_eq!(
            get("[palette]\nfloor = 7\n"),
            (
                Floor::Lift {
                    trigger: 7.0,
                    target: 7.0
                },
                0
            ),
            "a trigger above 4.5 lifts to itself"
        );
        for bad in ["0.5", "30", "\"thick\""] {
            assert_eq!(
                get(&format!("[palette]\nfloor = {bad}\n")),
                (Floor::DEFAULT, 1),
                "{bad}"
            );
        }
        let (f, said) = get("floor = \"off\"\n");
        assert_eq!(f, Floor::DEFAULT, "a top-level floor is not the palette's");
        assert_eq!(said, 1, "and it is said");
    }

    /// `[palette] selection` and `selection_text`: a colour as an `OSC`
    /// names one, or `"none"`; left out, the theme's; anything else is
    /// said and leaves the theme's.
    #[test]
    fn the_selection_keys_take_a_colour_or_none() {
        let get = |t: &str| {
            let mut c = Config::defaults();
            let said = c.apply(t);
            (c.selection, said.len())
        };
        let rgb = |r, g, b| {
            SelectionPick::Colour(baud_render::Argb::rgb(
                baud_domain::Chan::from_u8(r),
                baud_domain::Chan::from_u8(g),
                baud_domain::Chan::from_u8(b),
            ))
        };
        assert_eq!(get(""), (Selection::default(), 0));
        assert_eq!(
            get("[palette]\nselection = \"#3f638b\"\nselection_text = \"none\"\n"),
            (
                Selection {
                    page: rgb(0x3f, 0x63, 0x8b),
                    text: SelectionPick::None
                },
                0
            )
        );
        assert_eq!(
            get("[palette]\nselection_text = \"white\"\n").0.text,
            rgb(255, 255, 255),
            "an X11 name"
        );
        let (got, said) = get("[palette]\nselection = \"blueish\"\n");
        assert_eq!((got.page, said), (SelectionPick::Theme, 1));
        let (got, said) = get("selection = \"#3f638b\"\n");
        assert_eq!((got, said), (Selection::default(), 1), "the palette's key");
    }

    /// `[palette] cursor` and `cursor_text` read as the selection's keys:
    /// a colour as an `OSC` names one, or `"none"`; left out, the theme's;
    /// anything else said, the theme's kept.
    #[test]
    fn the_cursor_keys_take_a_colour_or_none() {
        let get = |t: &str| {
            let mut c = Config::defaults();
            let said = c.apply(t);
            (c.cursor, said.len())
        };
        assert_eq!(get(""), (CursorColours::default(), 0));
        assert_eq!(
            get("[palette]\ncursor = \"orange\"\ncursor_text = \"none\"\n"),
            (
                CursorColours {
                    fill: SelectionPick::Colour(baud_render::Argb::rgb(
                        baud_domain::Chan::from_u8(255),
                        baud_domain::Chan::from_u8(165),
                        baud_domain::Chan::from_u8(0),
                    )),
                    text: SelectionPick::None
                },
                0
            )
        );
        let (got, said) = get("[palette]\ncursor_text = \"reddish\"\n");
        assert_eq!((got.text, said), (SelectionPick::Theme, 1));
        let (got, said) = get("[cursor]\ncursor = \"#ffffff\"\n");
        assert_eq!(
            (got, said),
            (CursorColours::default(), 1),
            "the palette's key"
        );
    }

    /// `[palette] light` and `dark` are stored as written, quotes off and
    /// `~` kept for `palette.rs` to expand; they belong to `[palette]`, not
    /// the top level.
    #[test]
    fn the_palette_names_its_two_halves() {
        let mut c = Config::defaults();
        assert_eq!(
            (c.palette_light.as_deref(), c.palette_dark.as_deref()),
            (None, None)
        );
        let said = c.apply("[palette]\nlight = \"~/t/day.yaml\"\ndark = \"night\"\n");
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(c.palette_light.as_deref(), Some("~/t/day.yaml"));
        assert_eq!(c.palette_dark.as_deref(), Some("night"));
        let mut c = Config::defaults();
        let said = c.apply("light = \"day\"\ndark = \"night\"\n");
        assert_eq!(
            (c.palette_light, c.palette_dark),
            (None, None),
            "not the top level's"
        );
        assert_eq!(said.len(), 2, "and it is said: {said:?}");
    }

    /// The default stack starts with the embedded Regular -- JetBrains Mono
    /// with baud's Hebrew -- and the icon face, and on macOS keeps Menlo right
    /// behind them; the embedded STIX Two Math ends the chain; the three
    /// embedded styles are its styled faces, out of the chain. A config's
    /// `fonts` replaces the stack, and the embedded Regular, the icon face
    /// and STIX follow it.
    #[test]
    fn the_default_stack_starts_with_the_embedded_font() {
        use crate::embedded::{BOLD, BOLD_ITALIC, ITALIC, REGULAR, SYMBOLS};
        use baud_render::{FaceRole, FaceStyle};
        let d = super::without_badge(Config::defaults());
        assert!(is(&d.faces[0], REGULAR), "{}", d.faces[0].path);
        assert!(is(&d.faces[1], SYMBOLS), "{}", d.faces[1].path);
        #[cfg(target_os = "macos")]
        assert!(
            d.faces[2].from == FaceFrom::Family && d.faces[2].path == "Menlo",
            "{}",
            d.faces[2].path
        );
        let n = d.faces.len();
        assert!(
            is(&d.faces[n - 4], crate::embedded::STIX),
            "{}",
            d.faces[n - 4].path
        );
        for (f, e, style) in [
            (&d.faces[n - 3], BOLD, FaceStyle::Bold),
            (&d.faces[n - 2], ITALIC, FaceStyle::Italic),
            (&d.faces[n - 1], BOLD_ITALIC, FaceStyle::BoldItalic),
        ] {
            assert!(is(f, e), "{}", f.path);
            assert_eq!(f.role, FaceRole::Style(style), "{}", f.path);
        }
        assert!(
            d.faces[..n - 3]
                .iter()
                .all(|f| f.role == FaceRole::Fallback),
            "only the three styles are styled"
        );
        let mut c = Config::defaults();
        let said = c.apply("fonts = [\"/a.ttf\"]\n");
        assert!(said.is_empty(), "{said:?}");
        let c = super::without_badge(c);
        assert_eq!(c.faces.len(), 4);
        assert!(matches!(c.faces[0].from, FaceFrom::File));
        assert!(is(&c.faces[1], REGULAR));
        assert!(is(&c.faces[2], SYMBOLS));
        assert!(is(&c.faces[3], crate::embedded::STIX));
    }

    /// Whether `f` is the embedded font `e`, by its bytes.
    fn is(f: &FaceSpec, e: crate::embedded::Embedded) -> bool {
        matches!(f.from, FaceFrom::Embedded(b) if b == e.bytes)
    }

    fn is_symbols(f: &FaceSpec) -> bool {
        is(f, crate::embedded::SYMBOLS)
    }

    /// A configured stack keeps the embedded font behind it -- the Regular,
    /// for the Hebrew and whatever else the named faces lack and it maps,
    /// then the icon face, so a Nerd Fonts icon still draws (and STIX after
    /// it, below); and `font_fallback = false` leaves the text's stack
    /// exactly as named, whichever of the two keys comes first.
    #[test]
    fn a_configured_stack_keeps_the_embedded_font_behind_it() {
        let read = |text: &str| super::without_badge(read(text));
        let c = read("fonts = [\"/a.ttf\", \"/b.ttc@2\"]\n");
        assert_eq!(c.faces.len(), 5);
        assert_eq!(c.faces[0].path, "/a.ttf");
        assert_eq!(c.faces[1].path, "/b.ttc");
        assert!(
            is(&c.faces[2], crate::embedded::REGULAR),
            "{}",
            c.faces[2].path
        );
        assert!(is_symbols(&c.faces[3]), "{}", c.faces[3].path);
        assert!(
            is(&c.faces[4], crate::embedded::STIX),
            "{}",
            c.faces[4].path
        );
        assert_eq!(c.font_fallback, FontFallback::Embedded);
        for text in [
            "fonts = [\"/a.ttf\"]\nfont_fallback = false\n",
            "font_fallback = false\nfonts = [\"/a.ttf\"]\n",
        ] {
            let c = read(text);
            assert_eq!(c.font_fallback, FontFallback::Off, "{text}");
            assert_eq!(c.faces.len(), 1, "{text}");
            assert_eq!(c.faces[0].path, "/a.ttf", "{text}");
        }
        let c = read("font_fallback = true\nfonts = [\"/a.ttf\"]\n");
        assert_eq!(c.faces.len(), 4);
        assert!(is_symbols(&c.faces[2]));
    }

    /// **The embedded Regular follows a configured stack** -- the owner's
    /// order, configured faces, then baud's embedded font, then the system,
    /// which is not built for a configured stack yet (note AC decision 9)
    /// -- after the named faces and ahead of the icon face and STIX, as a
    /// fallback face and never a styled one, and under the same switch:
    /// `font_fallback = false` takes it away with the other two.
    #[test]
    fn the_embedded_regular_follows_a_configured_stack_before_the_icons() {
        let read = |text: &str| super::without_badge(read(text));
        let c = read("fonts = [\"/a.ttf\"]\n");
        assert!(matches!(c.faces[0].from, FaceFrom::File));
        assert!(
            is(&c.faces[1], crate::embedded::REGULAR),
            "{}",
            c.faces[1].path
        );
        assert_eq!(c.faces[1].role, baud_render::FaceRole::Fallback);
        assert!(is_symbols(&c.faces[2]), "{}", c.faces[2].path);
        assert!(
            is(&c.faces[3], crate::embedded::STIX),
            "{}",
            c.faces[3].path
        );
        assert_eq!(c.faces.len(), 4);
        let c = read("fonts = [\"/a.ttf\"]\nfont_fallback = false\n");
        assert!(
            c.faces.iter().all(|f| !is(f, crate::embedded::REGULAR)),
            "the switch turns the embedded Regular off too"
        );
    }

    /// The owner's call of 2026-10-03: the symbol face follows a configured
    /// stack as the icon face does, after it and under the same switch, so a
    /// stack whose faces lack Claude Code's arrow and bullet still draws them,
    /// and `font_fallback = false` still leaves the text's stack exactly as
    /// named.
    #[test]
    fn a_configured_stack_keeps_the_symbol_face_last() {
        let read = |text: &str| super::without_badge(read(text));
        let c = read("fonts = [\"/a.ttf\"]\n");
        assert_eq!(c.faces.len(), 4);
        assert!(is_symbols(&c.faces[2]), "{}", c.faces[2].path);
        assert!(
            is(&c.faces[3], crate::embedded::STIX),
            "{}",
            c.faces[3].path
        );
        let c = read("fonts = [\"/a.ttf\"]\nfont_fallback = false\n");
        assert!(
            c.faces.iter().all(|f| !is(f, crate::embedded::STIX)),
            "the switch turns the symbol face off too"
        );
    }

    /// With no `fonts` the embedded faces are already in the stack, so the switch
    /// changes nothing: it cannot take away the default font.
    #[test]
    fn font_fallback_alone_keeps_the_default_stack() {
        let c = read("font_fallback = false\n");
        assert_eq!(c.faces.len(), Config::defaults().faces.len());
        assert!(is(&c.faces[0], crate::embedded::REGULAR));
        assert!(is_symbols(&c.faces[1]));
    }

    /// A value that is not `true` or `false` is said and leaves the
    /// fallback on: a typo must not drop the icons.
    #[test]
    fn font_fallback_reads_true_and_false_and_says_anything_else() {
        let mut c = Config::defaults();
        let said = c.apply("font_fallback = off\nfonts = [\"/a.ttf\"]\n");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("font_fallback"), "{said:?}");
        assert_eq!(c.font_fallback, FontFallback::Embedded);
        assert!(is_symbols(&c.faces[2]));
    }

    /// Every theme the macOS app builds carries the floor it was given. The
    /// Linux driver sets it on its one theme at launch, by reading.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_built_theme_carries_the_configured_floor() {
        let glass = Glass::defaults();
        for floor in [baud_render::Floor::Off, baud_render::Floor::DEFAULT] {
            let pair = crate::app::themes_for_test(&glass, floor);
            assert_eq!(pair, (floor, floor));
        }
    }

    /// Each theme the macOS app builds takes the scheme named for its half,
    /// and only that one: the light file does not reach the dark theme.
    #[cfg(target_os = "macos")]
    #[test]
    fn each_half_takes_its_own_file() {
        use baud_domain::themefile::base16;
        let day = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/themes/day.yaml"
        ))
        .unwrap();
        let scheme = base16(&day).unwrap();
        let halves = crate::palette::Halves {
            dark: None,
            light: Some(scheme),
            selection: Selection::default(),
            cursor: CursorColours::default(),
        };
        let (dark, light) = crate::app::themes_with(&halves);
        let mut want = baud_render::Theme::light();
        want.take_scheme(&scheme);
        assert_eq!(light.palette, want.palette);
        assert_eq!((light.fg, light.bg), (want.fg, want.bg));
        assert_ne!(
            light.palette,
            baud_render::Theme::light().palette,
            "the file changed it"
        );
        assert_eq!(dark.palette, baud_render::Theme::dark().palette);
        assert_eq!(
            (dark.fg, dark.bg),
            (baud_render::Theme::dark().fg, baud_render::Theme::dark().bg)
        );
    }

    /// A mistyped alpha is clamped and said, not fatal and not silent --
    /// and a value that is not a number falls back to *that key's*
    /// default rather than to a shared zero, which would turn a typo into
    /// a missing material.
    #[test]
    fn a_value_outside_the_unit_range_is_clamped_and_a_word_is_refused() {
        let c = read("[glass]\ndim_dark = 4\ndim_light = -1\ntint_dark = \"thick\"\n");
        assert_eq!(c.glass.dim_dark, 1.0);
        assert_eq!(c.glass.dim_light, 0.0);
        assert_eq!(c.glass.tint_dark, 0.54, "a word fell back to zero");
    }

    /// An unknown word for an enum keeps the default rather than the
    /// previous value, so a typo cannot leave a config half-applied.
    #[test]
    fn an_unknown_material_or_shadow_falls_back() {
        let c = read("[glass]\nstyle = \"frosted\"\nshadow = \"sometimes\"\n");
        assert_eq!(c.glass.style, Glass::defaults().style);
        assert_eq!(c.glass.shadow, Shadow::Auto);
    }

    /// **The standard material is the default and Liquid Glass is a word
    /// away** (the owner, 2026-10-18: "use the recommendations"): a config
    /// that names `regular` or `clear` keeps the glass it asked for, and
    /// `standard` names the default.
    #[test]
    fn style_selects_the_standard_material_or_either_glass() {
        assert_eq!(read("").glass.style, Material::Standard);
        for (word, want) in [
            ("standard", Material::Standard),
            ("regular", Material::Regular),
            ("clear", Material::Clear),
        ] {
            let c = read(&format!("[glass]\nstyle = \"{word}\"\n"));
            assert_eq!(c.glass.style, want, "{word}");
        }
    }

    /// `background_alpha` is recognised only to be reported gone. The
    /// assertion with teeth is that it does not reach a dim: a reader's
    /// 255 must not turn into an opaque window through some later
    /// aliasing of the old key onto the new one.
    #[test]
    fn background_alpha_no_longer_reaches_a_dim() {
        let c = read("background_alpha = 255\nfont_size = 20\n");
        assert_eq!(c.glass.dim_dark, Glass::defaults().dim_dark);
        assert_eq!(c.glass.dim_light, Glass::defaults().dim_light);
        assert_eq!(c.font_size, 20.0, "the keys after it stopped being read");
    }

    /// `emoji` reads two words, and anything else is `font` and said.
    #[test]
    fn emoji_is_font_unless_named() {
        use baud_render::EmojiDefault;
        assert_eq!(read("").emoji, EmojiDefault::Font);
        assert_eq!(read("emoji = \"system\"").emoji, EmojiDefault::System);
        assert_eq!(read("emoji = \"font\"").emoji, EmojiDefault::Font);
        let mut c = Config::defaults();
        let said = c.apply("emoji = \"colour\"");
        assert_eq!(c.emoji, EmojiDefault::Font);
        assert_eq!(said.len(), 1, "a bad value is said: {said:?}");
        assert_eq!(
            read("[glass]\nemoji = \"system\"").emoji,
            EmojiDefault::Font,
            "a section's key of the same name is not this one"
        );
    }

    /// `bell` reads three words, and anything else is `sound` and said.
    #[test]
    fn bell_is_sound_unless_named() {
        assert_eq!(read("").bell, BellStyle::Sound);
        assert_eq!(read("bell = \"flash\"").bell, BellStyle::Flash);
        assert_eq!(read("bell = \"none\"").bell, BellStyle::Nothing);
        assert_eq!(read("bell = \"sound\"").bell, BellStyle::Sound);
        let mut c = Config::defaults();
        c.bell = BellStyle::Flash;
        let said = c.apply("bell = \"loud\"");
        assert_eq!(c.bell, BellStyle::Sound);
        assert_eq!(said.len(), 1, "a bad value is said: {said:?}");
        assert_eq!(
            read("[glass]\nbell = \"none\"").bell,
            BellStyle::Sound,
            "a section's key of the same name is not this one"
        );
    }

    /// `vs16_width` reads two words, and anything else is `two` and said.
    #[test]
    fn vs16_width_is_two_unless_named() {
        use baud_domain::Vs16Width;
        assert_eq!(read("").vs16_width, Vs16Width::Two);
        assert_eq!(read("vs16_width = \"one\"").vs16_width, Vs16Width::One);
        assert_eq!(read("vs16_width = \"two\"").vs16_width, Vs16Width::Two);
        let mut c = Config::defaults();
        let said = c.apply("vs16_width = \"three\"");
        assert_eq!(c.vs16_width, Vs16Width::Two);
        assert_eq!(said.len(), 1, "a bad value is said: {said:?}");
        assert_eq!(
            read("[glass]\nvs16_width = \"one\"").vs16_width,
            Vs16Width::Two,
            "a section's key of the same name is not this one"
        );
    }

    /// `[text] ambiguous_width` reads two words, quoted or bare; anything
    /// else is `narrow` and said, and the key outside `[text]` is not this
    /// one.
    #[test]
    fn ambiguous_width_is_narrow_unless_wide() {
        use baud_domain::AmbiguousWidth;
        assert_eq!(read("").ambiguous_width, AmbiguousWidth::Narrow);
        for (text, want) in [
            ("[text]\nambiguous_width = \"wide\"\n", AmbiguousWidth::Wide),
            ("[text]\nambiguous_width = wide\n", AmbiguousWidth::Wide),
            (
                "[text]\nambiguous_width = \"narrow\"\n",
                AmbiguousWidth::Narrow,
            ),
        ] {
            let mut c = Config::defaults();
            let said = c.apply(text);
            assert_eq!(c.ambiguous_width, want, "{text:?}");
            assert!(said.is_empty(), "{text:?}: {said:?}");
        }
        for text in [
            "[text]\nambiguous_width = \"double\"\n",
            "ambiguous_width = \"wide\"\n",
            "[glass]\nambiguous_width = \"wide\"\n",
        ] {
            let mut c = Config::defaults();
            let said = c.apply(text);
            assert_eq!(c.ambiguous_width, AmbiguousWidth::Narrow, "{text:?}");
            assert_eq!(said.len(), 1, "{text:?} is said: {said:?}");
        }
    }

    /// `font_smoothing` is recognised only to be reported gone, and the
    /// keys after it are still read.
    #[test]
    fn font_smoothing_is_reported_gone() {
        let mut c = Config::defaults();
        let said = c.apply("font_smoothing = \"on\"\nfont_size = 20\n");
        assert_eq!(said.len(), 1, "the removed key is said: {said:?}");
        assert!(said[0].contains("font_smoothing"), "{said:?}");
        assert_eq!(c.font_size, 20.0, "the keys after it stopped being read");
    }

    /// **The committed example must parse to what it documents**, and
    /// the first draft of it did not: `fonts` sat after the `[glass]`
    /// header, so the namespace this change introduces swallowed the font
    /// stack and the example silently ran the built-in one. A file whose
    /// keys are in the wrong order is the same defect as a key in the
    /// wrong section, and nothing but reading it catches either.
    #[test]
    fn the_committed_example_parses_to_what_it_documents() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../config.toml.example"
        ))
        .expect("config.toml.example");
        // No line a reload would refuse, so a copy of it shows no notice.
        assert_eq!(super::syntax(&text), None);
        let c = read(&text);
        assert_eq!(c.font_size, 13.0);
        assert_eq!(c.padding, 8.0);
        assert_eq!(c.config_reload, ConfigReload::On);
        assert_eq!(c.theme, ThemeChoice::Auto);
        assert_eq!(c.colorspace, Space::Srgb);
        // Its `[glass]` section is written at the defaults it describes.
        assert_eq!(c.glass, Glass::defaults());
        // Its `fonts` examples are commented out -- an uncommented one would
        // make a copied example replace the embedded default -- so the
        // stack is the built-in one, embedded face first.
        assert_eq!(c.faces.len(), Config::defaults().faces.len());
        assert!(
            is(&c.faces[0], crate::embedded::REGULAR),
            "{}",
            c.faces[0].path
        );
        assert_eq!(c.font_fallback, FontFallback::Embedded);
        // `bell` is written at its default, so the line is checked as well
        // as what it reads to.
        assert!(text.contains("\nbell = \"sound\"\n"), "the bell line");
        assert_eq!(c.bell, BellStyle::Sound);
        // The `[cursor]` section lands; that a section header cannot
        // swallow a top-level list is `a_list_across_lines_survives_a_section_header`.
        // The example's `trail` line is commented out -- naming a trail keeps
        // it under Reduce Motion -- so the section's landing is the shape and
        // blink below, which are named.
        assert_eq!(c.cursor_trail, TrailPick::Default);
        // What it says the default is, it is: the prose and the commented
        // line both name the trail the reader runs by default.
        let default = trail_name(CursorTrail::default());
        assert!(
            text.contains(&format!("\"{default}\", the default,")),
            "config.toml.example calls another trail the default"
        );
        assert!(
            text.contains(&format!("\n# trail = \"{default}\"\n")),
            "config.toml.example's commented trail is not the default"
        );
        assert_eq!(
            (c.blink_animation, c.blink_interval),
            (BlinkAnimation::Plasma, BlinkInterval::Default),
            "the example's blink animation is the default and its interval unset"
        );
        assert!(
            text.contains("\nblink_animation = \"plasma\"\n"),
            "the animation line"
        );
        assert!(
            text.contains("\n# blink_interval_ms = 333\n"),
            "the interval line"
        );
        assert_eq!(
            (
                c.cursor_shape,
                c.cursor_blink,
                c.cursor_stop,
                c.trail_scroll
            ),
            (
                CursorShape::Block,
                BlinkChoice::Auto,
                StopAfter::Never,
                TrailScroll::Ride
            )
        );
        assert_eq!(
            c.screen_transition,
            ScreenTransition::Off,
            "the [screen] section did not land"
        );
        assert!(
            text.contains(
                "\n[screen]\ntransition = false\nchecksum = \"on\"\nlogging = \"on\"\nprinting = \"on\"\n"
            ),
            "the [screen] checksum, logging and printing"
        );
        assert_eq!(c.checksum, baud_domain::ChecksumReport::On);
        assert_eq!(c.logging, baud_domain::Logging::On);
        assert_eq!(c.printing, Printing::On);
        assert!(
            text.contains("\nnotifications = \"on\"\n"),
            "the [screen] notifications"
        );
        assert_eq!(c.notifications, Notifications::On);
        assert!(
            text.contains("\nexit_status = \"off\"\n"),
            "the [screen] exit_status"
        );
        assert_eq!(c.exit_status, baud_render::StatusMarks::Off);
        assert!(
            text.contains("\n[window]\noperations = \"on\"\n"),
            "the [window] operations"
        );
        assert_eq!(c.window_ops, baud_domain::WindowOps::On);
        assert!(
            text.contains("\n[pointer]\nhide_while_typing = \"on\"\n"),
            "the [pointer] hide_while_typing"
        );
        assert_eq!(c.pointer_mode, baud_domain::PointerMode::UnlessTracking);
        assert!(
            text.contains("\n[font]\nchanges = \"on\"\n"),
            "the [font] changes"
        );
        assert_eq!(c.font_changes, baud_domain::FontChanges::On);
        assert_eq!(
            c.scroll,
            ScrollMode::Smooth,
            "the [scroll] section did not land"
        );
        assert!(
            text.contains("\n[text]\nambiguous_width = \"narrow\"\n"),
            "the [text] section"
        );
        assert_eq!(c.ambiguous_width, baud_domain::AmbiguousWidth::Narrow);
        // The values above are all defaults, so they read the same whether
        // a line landed or was swallowed by the section above it. What
        // separates the two is the reader's own account: every key the
        // example writes reached an arm that acts on it, in the section it
        // sits in, and no value fell back.
        let said = Config::defaults().apply(&text);
        assert!(
            said.is_empty(),
            "the example writes what the reader ignores: {said:?}"
        );
        // And the reader saw every key line. A line swallowed into an
        // unclosed list, or dropped by the tokenizer, is nothing said; this
        // counts them a second way, as a key at the start of a line.
        let written = text
            .lines()
            .filter(|l| {
                l.split_once(" =").is_some_and(|(k, _)| {
                    !k.is_empty()
                        && k.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                })
            })
            .count();
        assert!(
            written > 10,
            "the key count found {written} lines; the probe is broken"
        );
        assert_eq!(
            fields(&text).len(),
            written,
            "a key line did not reach the reader"
        );
        assert_eq!(c.glass, Glass::defaults(), "a [glass] key did not land");
        // The `[images]` section lands too, and it sits after `fonts` for
        // the reason `[cursor]` does -- a section before the stack would
        // swallow it, which the face assert above would catch.
        assert_eq!(
            c.images,
            crate::host::IMAGE_BUDGET,
            "the [images] section did not land"
        );
        assert_eq!(
            c.clipboard,
            Clipboard::DEFAULT,
            "the [clipboard] section did not land"
        );
    }

    /// The list reader still spans lines with a section above it, which
    /// is the one shape the header test could have broken: a list's `[`
    /// opens a value where a header's `[` opens a line.
    #[test]
    fn a_list_across_lines_survives_a_section_header() {
        let c = read("[glass]\ndim_dark = 0.3\n\nfonts = [\n  \"a\",\n  \"b@2\",\n]\n");
        // `fonts` is under `[glass]` here, so it is NOT the font stack --
        // which is the namespace working, not a defect.
        assert_eq!(c.faces.len(), Config::defaults().faces.len());
        let c = super::without_badge(read(
            "fonts = [\n  \"/a.ttf\",\n  \"/b.ttc@2\",\n]\n[glass]\ndim_dark = 0.3\n",
        ));
        assert_eq!(
            c.faces.len(),
            5,
            "the two named and the three embedded fallbacks"
        );
        assert_eq!(c.faces[1].path, "/b.ttc");
        assert_eq!(c.faces[1].index, baud_font::FaceIndex::of(2));
        assert!(is_symbols(&c.faces[3]));
        assert_eq!(c.glass.dim_dark, 0.3);
    }
}

/// **Where the config is looked for, and what the paths in it resolve
/// against.** `Config::launch` reads the environment and opens paths against
/// the working directory, and both are the whole process's: a test that set
/// either would move every other test running beside it. So each case runs
/// [`load_helper`] as a process of its own, with the directory and the
/// variables it names, and reads back what the load found.
#[cfg(test)]
mod lookup {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    /// **The lookup's order, and nothing under the working directory**:
    /// `BAUD_CONFIG`, then XDG's, then `~/.config`'s, each only when it is
    /// there, and a directory variable only when it is absolute.
    #[test]
    fn the_lookup_is_three_places_and_no_relative_one() {
        let p = |s: &str| Some(PathBuf::from(s));
        assert_eq!(
            super::config_paths(p("/b/c.toml"), p("/x"), p("/h")),
            [
                PathBuf::from("/b/c.toml"),
                PathBuf::from("/x/baud/config.toml"),
                PathBuf::from("/h/.config/baud/config.toml"),
            ]
        );
        assert!(super::config_paths(None, None, None).is_empty());
        let dir = |s: &str| super::absolute_dir(Some(s.into()));
        assert_eq!(dir("/x"), p("/x"));
        assert_eq!(dir(""), None, "empty is unset");
        assert_eq!(dir("rel"), None, "relative is ignored");
        assert_eq!(super::absolute_dir(None), None);
    }

    /// The variable that turns [`load_helper`] on.
    const LOAD_HELPER: &str = "BAUD_TEST_LOAD_HELPER";

    /// **Run only by the tests below**, as a process of its own; a no-op
    /// otherwise. It reads the config as the app does, before its family
    /// names are resolved against the installed fonts, and prints the file
    /// it read, every face read from a file, and where a theme named
    /// `night` would be looked for.
    #[test]
    fn load_helper() {
        if std::env::var_os(LOAD_HELPER).is_none() {
            return;
        }
        let c = super::Config::read();
        match &c.source {
            Some(p) => println!("SOURCE[{}]", p.display()),
            None => println!("SOURCE[none]"),
        }
        for f in &c.faces {
            if matches!(f.from, super::FaceFrom::File) {
                println!("FACE[{}@{:?}]", f.path, f.index);
            }
        }
        let dirs = crate::palette::Dirs::from_env(c.source.as_deref());
        for p in crate::palette::candidates("night", &dirs) {
            println!("THEME[{}]", p.display());
        }
    }

    /// A directory of this test's own, made fresh and canonical: on macOS
    /// the temporary directory sits behind the `/var` symlink, and a path
    /// the child reports is the resolved one.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baud-lookup-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch directory");
        d.canonicalize().expect("it resolves")
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its directory");
        std::fs::write(path, text).expect("the file");
    }

    /// What [`load_helper`] printed, run in `dir` with `HOME` set to
    /// `home`, no `BAUD_CONFIG` or `XDG_CONFIG_HOME` but those `env` sets.
    fn load_in(dir: &Path, home: &Path, env: &[(&str, &OsStr)]) -> Vec<String> {
        let mut cmd = std::process::Command::new(std::env::current_exe().expect("this test"));
        cmd.args(["--exact", "config::lookup::load_helper", "--nocapture"])
            .current_dir(dir)
            .env_remove("BAUD_CONFIG")
            .env_remove("XDG_CONFIG_HOME")
            .env("HOME", home)
            .env(LOAD_HELPER, "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("the helper runs");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "the helper passed: {text}");
        let lines: Vec<String> = text
            .lines()
            .filter(|l| {
                ["SOURCE[", "FACE[", "THEME["]
                    .iter()
                    .any(|p| l.starts_with(p))
            })
            .map(str::to_string)
            .collect();
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("SOURCE[")).count(),
            1,
            "the helper loaded once: {text}"
        );
        lines
    }

    fn source(lines: &[String]) -> &str {
        lines
            .iter()
            .find_map(|l| l.strip_prefix("SOURCE[")?.strip_suffix(']'))
            .expect("a source line")
    }

    /// **The working directory is not searched**, the owner's call of
    /// 2026-10-13: a `config.toml` in the directory baud starts in is not
    /// read. The control names the same file through `BAUD_CONFIG` and it
    /// is read, so the file is one the load can find.
    #[test]
    fn the_working_directory_is_not_searched_for_a_config() {
        let dir = scratch("cwd");
        let home = scratch("cwd-home");
        write(&dir.join("config.toml"), "font_size = 21\n");
        let named = dir.join("config.toml");
        let control = load_in(&dir, &home, &[("BAUD_CONFIG", named.as_os_str())]);
        assert_eq!(source(&control), named.display().to_string(), "the control");
        assert_eq!(source(&load_in(&dir, &home, &[])), "none");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **An empty or relative `XDG_CONFIG_HOME` is ignored**, and the
    /// lookup goes on to `~/.config`: "If $XDG_CONFIG_HOME is either not
    /// set or empty, a default equal to $HOME/.config should be used", and
    /// "If an implementation encounters a relative path in any of these
    /// variables it should consider the path invalid and ignore it"
    /// (`[[source: xdg-basedir-0-8]]`). Both would otherwise name a file
    /// under the working directory, and one is put there for each. The
    /// control is the same directory named absolutely, which is read; no
    /// theme is looked for under a relative directory either.
    #[test]
    fn an_empty_or_relative_xdg_config_home_is_ignored() {
        let dir = scratch("xdg");
        let home = scratch("xdg-home");
        write(&dir.join("baud/config.toml"), "font_size = 21\n");
        write(&dir.join("rel/baud/config.toml"), "font_size = 22\n");
        let homed = home.join(".config/baud/config.toml");
        write(&homed, "font_size = 23\n");
        let control = load_in(&dir, &home, &[("XDG_CONFIG_HOME", dir.as_os_str())]);
        assert_eq!(
            source(&control),
            dir.join("baud/config.toml").display().to_string(),
            "the control"
        );
        for xdg in ["", "rel"] {
            let got = load_in(&dir, &home, &[("XDG_CONFIG_HOME", OsStr::new(xdg))]);
            assert_eq!(
                source(&got),
                homed.display().to_string(),
                "{xdg:?}: {got:?}"
            );
            for l in &got {
                if let Some(p) = l.strip_prefix("THEME[") {
                    assert!(
                        p.starts_with('/'),
                        "{xdg:?}: a theme under the cwd: {got:?}"
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **A relative `BAUD_CONFIG` names a file under the directory baud
    /// was started in**, and the config in force is said by that whole
    /// path, so the `themes` beside it are looked for there too. baud's
    /// own directory never changes -- the shell is the one sent home -- so
    /// it is the directory the variable was written against.
    #[test]
    fn a_relative_baud_config_is_read_under_the_launch_directory() {
        let dir = scratch("relative");
        let home = scratch("relative-home");
        write(&dir.join("sub/c.toml"), "font_size = 21\n");
        let got = load_in(&dir, &home, &[("BAUD_CONFIG", OsStr::new("sub/c.toml"))]);
        assert_eq!(source(&got), dir.join("sub/c.toml").display().to_string());
        assert!(
            got.contains(&format!(
                "THEME[{}]",
                dir.join("sub/themes/night").display()
            )),
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **A relative font path is read beside the config file**, the
    /// owner's call of 2026-10-13, not under the directory baud started
    /// in; a collection's `@N` stays the index, and an absolute path is
    /// kept as written.
    #[test]
    fn a_relative_font_path_resolves_against_the_config_files_directory() {
        let dir = scratch("fonts");
        let launch = scratch("fonts-launch");
        let named = dir.join("config.toml");
        write(
            &named,
            "fonts = [\"f.ttf@1\", \"deep/g.otf\", \"/abs/h.ttf\"]\nfont_fallback = false\n",
        );
        let got = load_in(&launch, &launch, &[("BAUD_CONFIG", named.as_os_str())]);
        let faces: Vec<&String> = got.iter().filter(|l| l.starts_with("FACE[")).collect();
        assert_eq!(
            faces,
            [
                &format!("FACE[{}@FaceIndex(1)]", dir.join("f.ttf").display()),
                &format!("FACE[{}@FaceIndex(0)]", dir.join("deep/g.otf").display()),
                &"FACE[/abs/h.ttf@FaceIndex(0)]".to_string(),
            ],
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(launch);
    }

    /// **`~/` at the start of a font path is the home folder**, from
    /// `HOME`, as a theme path's is and as ghostty expands it: only `~/`,
    /// so `~user/` is a relative path like any other and is read beside
    /// the config. With `HOME` empty there is no home folder, and the path
    /// is kept as written for the launch to say it is not a file.
    #[test]
    fn a_font_path_under_home_expands_its_tilde() {
        let dir = scratch("tilde");
        let home = scratch("tilde-home");
        let named = dir.join("config.toml");
        write(
            &named,
            "fonts = [\"~/fonts/m.ttf@2\", \"~user/x.ttf\"]\nfont_fallback = false\n",
        );
        let faces = |got: &[String]| -> Vec<String> {
            got.iter()
                .filter(|l| l.starts_with("FACE["))
                .cloned()
                .collect()
        };
        let got = load_in(&dir, &home, &[("BAUD_CONFIG", named.as_os_str())]);
        assert_eq!(
            faces(&got),
            [
                format!("FACE[{}@FaceIndex(2)]", home.join("fonts/m.ttf").display()),
                format!("FACE[{}@FaceIndex(0)]", dir.join("~user/x.ttf").display()),
            ],
            "{got:?}"
        );
        let got = load_in(
            &dir,
            &home,
            &[("BAUD_CONFIG", named.as_os_str()), ("HOME", OsStr::new(""))],
        );
        assert_eq!(
            faces(&got)[0],
            "FACE[~/fonts/m.ttf@FaceIndex(2)]",
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(home);
    }
}

/// The user guide's configuration page, held to the reader.
#[cfg(test)]
mod guide {
    use std::collections::BTreeSet;

    /// Every `(section, key)` the reader matches by name, read from
    /// `Config::apply`'s own arms: a line in that function that opens with
    /// `("` and holds `") =>`. The catch-all arms bind a name rather than a
    /// literal, so they are not read as keys.
    fn reader_keys() -> BTreeSet<(String, String)> {
        let src = include_str!("config.rs");
        let start = src.find("fn apply(").expect("Config::apply");
        let body = &src[start..];
        let end = body
            .find("if let Some(mut faces) = named")
            .expect("the end of apply's match");
        body[..end]
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("(\"") && l.contains("\") =>"))
            .filter_map(|l| {
                let mut q = l.split('"');
                let section = q.nth(1)?.to_string();
                let key = q.nth(1)?.to_string();
                Some((section, key))
            })
            .collect()
    }

    /// Every `(section, key)` docs/guide/configuration.md lists: a table row
    /// opening with a backticked key under `## Top level` or `## [section]`,
    /// and under `## Keys baud no longer reads` a backticked key that may
    /// carry its `[section]` in front.
    fn documented_keys(page: &str) -> BTreeSet<(String, String)> {
        enum In {
            Section(String),
            Gone,
            Elsewhere,
        }
        let mut at = In::Elsewhere;
        let mut keys = BTreeSet::new();
        for line in page.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                at = if h == "Top level" {
                    In::Section(String::new())
                } else if let Some(s) = h.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
                    In::Section(s.to_string())
                } else if h == "Keys baud no longer reads" {
                    In::Gone
                } else {
                    In::Elsewhere
                };
                continue;
            }
            let Some(cell) = line
                .strip_prefix("| `")
                .and_then(|r| r.split_once('`'))
                .map(|(c, _)| c)
            else {
                continue;
            };
            match &at {
                In::Section(s) => {
                    keys.insert((s.clone(), cell.to_string()));
                }
                In::Gone => {
                    let (s, k) = match cell.strip_prefix('[').and_then(|c| c.split_once("] ")) {
                        Some((s, k)) => (s.to_string(), k.to_string()),
                        None => (String::new(), cell.to_string()),
                    };
                    keys.insert((s, k));
                }
                In::Elsewhere => {}
            }
        }
        keys
    }

    /// **docs/guide/configuration.md lists every key the reader acts on, and
    /// no other.** The example file is held to the reader in one direction
    /// (`the_committed_example_parses_to_what_it_documents`: every key it
    /// writes reaches an arm); this holds the user's page in both, so a key
    /// added to `apply` fails here until the page has its row, and a row for
    /// a key the reader never matches fails as an invented one. The positive
    /// controls are a first and a last arm and a retired key, which the scan
    /// must find on each side.
    #[test]
    fn the_configuration_page_lists_every_key_the_reader_knows() {
        let page = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/guide/configuration.md"
        ))
        .expect("docs/guide/configuration.md");
        let reader = reader_keys();
        let documented = documented_keys(&page);
        for side in [&reader, &documented] {
            for (s, k) in [
                ("", "font_size"),
                ("glass", "saturation"),
                ("images", "body_kb"),
            ] {
                assert!(
                    side.contains(&(s.to_string(), k.to_string())),
                    "a scan stopped matching: no [{s}] {k} in {side:?}"
                );
            }
        }
        let missing: Vec<_> = reader.difference(&documented).collect();
        let invented: Vec<_> = documented.difference(&reader).collect();
        assert!(
            missing.is_empty() && invented.is_empty(),
            "docs/guide/configuration.md is out of step with Config::apply:\n  \
             read but not documented: {missing:?}\n  documented but never read: {invented:?}"
        );
    }
}
