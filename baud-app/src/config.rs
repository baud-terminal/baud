//! The terminal's configuration: which faces, what size, how translucent.
//!
//! A small, dependency-free reader for a subset of TOML -- `key = value`,
//! string, integer and decimal scalars, a bracketed list of strings that may
//! span lines, and `[section]` headers. baud owns its stack rather than
//! pulling `serde` and a TOML crate for a handful of keys -- a font list,
//! `font_size`, `padding`, `colorspace`, `vs16_width`, `emoji`, `theme`, `bell`,
//! `[text] ambiguous_width`, and the eight under `[glass]` -- and a key it does not understand is ignored, and said,
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
//! a configured font), unless `font_fallback = false`. A configured stack has
//! no styled faces yet: naming them waits on fonts by family name.

use baud_domain::{CursorBlink, CursorShape};

/// A face is a file and, for a collection (`.ttc`), which face in it: the
/// path may carry `@N`, so `Menlo.ttc@0`. Or it is the font compiled into
/// the binary, whose `path` is only a name for the log.
pub struct FaceSpec {
    pub path: String,
    pub index: u32,
    pub from: FaceFrom,
    /// Part of the fallback chain, or the face one style is drawn from.
    pub role: baud_render::FaceRole,
}

/// Where a face's bytes come from.
#[derive(Clone, Copy)]
pub enum FaceFrom {
    /// A file on disk, mapped read-only at launch.
    File,
    /// Bytes compiled into the binary.
    Embedded(&'static [u8]),
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

pub struct Config {
    /// Where the file was found, so the startup log can say which config
    /// is in force -- a `baud` launched outside the checkout once fell back
    /// to the Menlo stack in silence, and looked like a font that had
    /// stopped being respected.
    pub source: Option<std::path::PathBuf>,
    /// The fallback stack, first face first.
    pub faces: Vec<FaceSpec>,
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
    /// `[screen] transition`: whether a switch to or from the alternate
    /// screen fades the new one in from the background. Off by default.
    pub screen_transition: ScreenTransition,
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
}

/// `[clipboard] osc52`: whether a program may write the clipboard with
/// `OSC 52`. **`Write` by default**, the owner's V11 (2026-09-25), as kitty
/// and sfte do; reads stay refused whatever this says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc52 {
    Write,
    Off,
}

/// The `[clipboard]` section.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Clipboard {
    pub osc52: Osc52,
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
/// the example's calibration.** The HIG says to use the regular material
/// "when components have a significant amount of text"
/// ([[source: apple-hig-materials]]), and a terminal is all text; the dims
/// are Jev's decided 0.70 and 0.60, which keep default text at or above
/// 4.5:1, the HIG's minimum ([[source: apple-hig-dark-mode]]), over flat
/// white and flat black with the material darkening nothing;
/// `the_default_dims_keep_text_readable_over_white_and_black` computes
/// that, and is a floor under them, not their derivation.
/// The tints, the hue, the saturation and the shadow are the example's,
/// which its header records as "tuned against the system widget over one
/// wallpaper rather than derived from a measurement" -- a starting point a
/// reader adjusts, which is why every value here is a key.
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
    /// `style`: which material. `Regular`, the default, is the one the HIG
    /// gives surfaces with "a significant amount of text"; `Clear` is the
    /// example's, which the HIG keeps for components "that appear over
    /// visually rich backgrounds" ([[source: apple-hig-materials]]).
    /// **Not Jev's pick**: Jev chose clear at 0.92 and 0.95 (decisions
    /// 716/717), on a request that modelled only clear and left regular
    /// unmeasured, and the owner asked for Apple's recommendations.
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

/// Which `NSGlassEffectView` style the surface asks for.
///
/// Named for what it selects rather than `GlassStyle`, which is the
/// crate's own type: this one has to compile on Linux, so it carries the
/// choice and the driver maps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Material {
    Clear,
    #[default]
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
    /// The regular material, dims of 0.70 and 0.60 -- where the example's
    /// `GN_GLASS` is clear, `GN_PAPER_D` 0.22 and `GN_PAPER_L` 0.12, which
    /// put default text at 1.14:1 and 1.13:1 over white and black -- and
    /// the rest the example's own `GN_*` defaults: `GN_GT_D` 0.54,
    /// `GN_GT_L` 0.14, `GN_HUE` 0.60, `GN_HSAT` 0.80, `GN_SHADOW` auto.
    pub fn defaults() -> Glass {
        Glass {
            style: Material::Regular,
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
    /// driver. Only the macOS driver has a trail, so Linux never asks.
    #[cfg(any(target_os = "macos", test))]
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
/// driver. It decides the default trail and holds `SGR 5` text shown.
/// Only the macOS driver has a trail or blinks text, so Linux never asks.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemMotion {
    Full,
    Reduced,
}

#[cfg(any(target_os = "macos", test))]
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
    #[cfg(any(target_os = "macos", test))]
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
            font_fallback: FontFallback::Embedded,
            font_size: 13.0,
            padding: 8.0,
            colorspace: Space::Srgb,
            theme: ThemeChoice::Auto,
            floor: baud_render::Floor::DEFAULT,
            palette_light: None,
            palette_dark: None,
            cursor_trail: TrailPick::Default,
            cursor_shape: CursorShape::Block,
            cursor_blink: BlinkChoice::Auto,
            cursor_stop: StopAfter::Never,
            blink_animation: BlinkAnimation::Plasma,
            blink_interval: BlinkInterval::Default,
            screen_transition: ScreenTransition::Off,
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
            source: None,
        }
    }

    /// The config from the first file that exists, over the defaults; the
    /// defaults themselves when none is found.
    pub fn load() -> Config {
        let mut c = Config::defaults();
        if let Some((path, text)) = read_first() {
            for said in c.apply(&text) {
                crate::warn!("{said}");
            }
            if let Some(dir) = path.parent() {
                c.faces_beside(dir);
            }
            c.source = Some(path);
        }
        c
    }

    /// **A relative font path names a file beside the config**, in `dir`,
    /// the directory the config file is in -- the owner's call of
    /// 2026-10-13 -- and not under the directory baud was started in, which
    /// from the Dock is `/`. An absolute path and a compiled-in face are
    /// kept. The `@N` of a collection was split off when the value was
    /// read, so only the path is joined. A path that `dir` would make
    /// unreadable as UTF-8 is kept as written.
    fn faces_beside(&mut self, dir: &std::path::Path) {
        for f in &mut self.faces {
            if matches!(f.from, FaceFrom::File) && std::path::Path::new(&f.path).is_relative() {
                if let Some(joined) = dir.join(&f.path).to_str() {
                    f.path = joined.to_string();
                }
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
                        "clear" => Material::Clear,
                        "regular" => Material::Regular,
                        other => {
                            said.push(format!(
                                "config: glass style {other:?} is not clear or regular; \
                                 using regular"
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
            // Regular only: a configured stack has no styled faces, so its
            // bold and italic Hebrew draws from this face too, upright and
            // regular weight.
            if self.font_fallback == FontFallback::Embedded {
                faces.push(FaceSpec::embedded(crate::embedded::REGULAR));
                faces.push(FaceSpec::embedded(crate::embedded::SYMBOLS));
                faces.push(FaceSpec::embedded(crate::embedded::STIX));
            }
            self.faces = faces;
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
    fn parse(s: &str) -> FaceSpec {
        match s
            .rsplit_once('@')
            .and_then(|(p, i)| Some((p, i.parse().ok()?)))
        {
            Some((path, index)) => FaceSpec {
                path: path.to_string(),
                index,
                from: FaceFrom::File,
                role: baud_render::FaceRole::Fallback,
            },
            None => FaceSpec {
                path: s.to_string(),
                index: 0,
                from: FaceFrom::File,
                role: baud_render::FaceRole::Fallback,
            },
        }
    }

    /// A font compiled into the binary, in the fallback chain.
    fn embedded(e: crate::embedded::Embedded) -> FaceSpec {
        FaceSpec {
            path: e.name.to_string(),
            index: 0,
            from: FaceFrom::Embedded(e.bytes),
            role: baud_render::FaceRole::Fallback,
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

/// The first config file that reads, by the path it was read at.
fn read_first() -> Option<(std::path::PathBuf, String)> {
    let given = std::env::var_os("BAUD_CONFIG")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p));
    config_paths(
        given,
        absolute_dir(std::env::var_os("XDG_CONFIG_HOME")),
        absolute_dir(std::env::var_os("HOME")),
    )
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
/// and Kohinoor for Devanagari, which the corpus carries too.
#[cfg(target_os = "macos")]
const DEFAULT_FACES: [&str; 6] = [
    "/System/Library/Fonts/Menlo.ttc@0",
    "/System/Library/Fonts/Apple Color Emoji.ttc@0",
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Hiragino Sans GB.ttc@0",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc@0",
    "/System/Library/Fonts/Kohinoor.ttc@0",
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

/// The embedded Regular and icon face first; then, since Linux has no one system
/// mono, the first of the usual DejaVu Sans Mono locations that exists, for
/// what JetBrains Mono does not map, Noto Color Emoji where it is, and the
/// embedded STIX Two Math last, as on macOS. A config names anything else.
#[cfg(target_os = "linux")]
fn default_faces() -> Vec<FaceSpec> {
    const MONO: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/run/current-system/sw/share/X11/fonts/DejaVuSansMono.ttf",
    ];
    const EMOJI: &[&str] = &[
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/google-noto-color-emoji-fonts/NotoColorEmoji.ttf",
        "/run/current-system/sw/share/X11/fonts/NotoColorEmoji.ttf",
    ];
    let first = |paths: &[&str]| {
        paths
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|s| FaceSpec::parse(s))
    };
    [
        FaceSpec::embedded(crate::embedded::REGULAR),
        FaceSpec::embedded(crate::embedded::SYMBOLS),
    ]
    .into_iter()
    .chain(first(MONO))
    .chain(first(EMOJI))
    .chain([FaceSpec::embedded(crate::embedded::STIX)])
    .chain(styled_faces())
    .collect()
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
    use baud_font::{Face, FaceIndex};

    /// Box drawing, block elements and braille are not in the list: baud
    /// draws those itself (`baud_render::boxes`), so no face has to carry
    /// them and the stack is not held to a range it never renders.
    #[test]
    fn the_default_stack_carries_han_kana_hangul_devanagari_and_emoji() {
        let specs = default_faces();
        let files: Vec<Vec<u8>> = specs
            .iter()
            .map(|f| match f.from {
                FaceFrom::Embedded(b) => b.to_vec(),
                FaceFrom::File => {
                    std::fs::read(&f.path).unwrap_or_else(|e| panic!("{}: {e}", f.path))
                }
            })
            .collect();
        let faces: Vec<Face<'_>> = files
            .iter()
            .zip(&specs)
            .map(|(d, f)| Face::parse_index(d, FaceIndex::of(f.index)).expect("a face"))
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
        assert_eq!(g.style, Material::Regular);
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
        assert_eq!(c.glass.style, Material::Regular, "top-level style leaked");
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
        let d = Config::defaults();
        assert!(is(&d.faces[0], REGULAR), "{}", d.faces[0].path);
        assert!(is(&d.faces[1], SYMBOLS), "{}", d.faces[1].path);
        #[cfg(target_os = "macos")]
        assert!(
            d.faces[2].path.ends_with("Menlo.ttc"),
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
    /// it, below); and `font_fallback = false` leaves the stack exactly as
    /// named, whichever of the two keys comes first.
    #[test]
    fn a_configured_stack_keeps_the_embedded_font_behind_it() {
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
    /// and `font_fallback = false` still leaves the stack exactly as named.
    #[test]
    fn a_configured_stack_keeps_the_symbol_face_last() {
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
        let c = read(&text);
        assert_eq!(c.font_size, 13.0);
        assert_eq!(c.padding, 8.0);
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
        let c = read("fonts = [\n  \"a\",\n  \"b@2\",\n]\n[glass]\ndim_dark = 0.3\n");
        assert_eq!(
            c.faces.len(),
            5,
            "the two named and the three embedded fallbacks"
        );
        assert_eq!(c.faces[1].path, "b");
        assert_eq!(c.faces[1].index, 2);
        assert!(is_symbols(&c.faces[3]));
        assert_eq!(c.glass.dim_dark, 0.3);
    }
}

/// **Where the config is looked for, and what the paths in it resolve
/// against.** `Config::load` reads the environment and opens paths against
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
    /// otherwise. It loads the config as the app does and prints the file
    /// it read, every face read from a file, and where a theme named
    /// `night` would be looked for.
    #[test]
    fn load_helper() {
        if std::env::var_os(LOAD_HELPER).is_none() {
            return;
        }
        let c = super::Config::load();
        match &c.source {
            Some(p) => println!("SOURCE[{}]", p.display()),
            None => println!("SOURCE[none]"),
        }
        for f in &c.faces {
            if matches!(f.from, super::FaceFrom::File) {
                println!("FACE[{}@{}]", f.path, f.index);
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
                &format!("FACE[{}@1]", dir.join("f.ttf").display()),
                &format!("FACE[{}@0]", dir.join("deep/g.otf").display()),
                &"FACE[/abs/h.ttf@0]".to_string(),
            ],
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(launch);
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
