//! The terminal's configuration: which faces, what size, how translucent.
//!
//! A small, dependency-free reader for a subset of TOML -- `key = value`,
//! string, integer and decimal scalars, a bracketed list of strings that may
//! span lines, and `[section]` headers. baud owns its stack rather than
//! pulling `serde` and a TOML crate for a handful of keys -- a font list,
//! `font_size`, `padding`, `colorspace`, `theme`, and the eight under
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
//! config.toml`, then `~/.config/baud/config.toml`, then `./config.toml`. When
//! none is found or a field is absent, the defaults are the built-in Menlo
//! stack -- so the app runs with no config at all.

use baud_domain::{CursorBlink, CursorShape};

/// A face is a file and, for a collection (`.ttc`), which face in it: the
/// path may carry `@N`, so `Menlo.ttc@0`.
pub struct FaceSpec {
    pub path: String,
    pub index: u32,
}

pub struct Config {
    /// Where the file was found, so the startup log can say which config
    /// is in force -- a `baud` launched outside the checkout once fell back
    /// to the Menlo stack in silence, and looked like a font that had
    /// stopped being respected.
    pub source: Option<std::path::PathBuf>,
    /// The fallback stack, first face first.
    pub faces: Vec<FaceSpec>,
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
    /// `[cursor] trail`: which trail a cursor move leaves -- see
    /// [`CursorTrail`] -- or the default, which is not the same thing:
    /// [`TrailPick`]. **On by default**, the owner's call on 2026-09-25,
    /// where sfte ships its own `SFTE_CURSOR_TRAIL` off: an idle terminal
    /// pays nothing for it, because only a cursor that moved spawns one.
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
    /// Two knobs, because the two limits are independent and a single one
    /// scaling both would rest on an unmeasured guess about Sixel encoding
    /// density -- a user who raised it and still hit a refusal would have no
    /// knob left to turn. `pixels_mb` bounds the *decoded* ARGB image (a
    /// 1024x1024 image is 4 MiB whatever its Sixel encodes to); `body_kb`
    /// bounds the *raw* Sixel body held before it decodes, which varies with
    /// its colour count and run-length use. A large image needs both raised:
    /// the default 4 MiB / 256 KiB holds a moderate Sixel, and either limit
    /// alone refuses (and counts) rather than truncating.
    pub images: baud_domain::ImageBudget,
    /// Whether the Primary DA advertises Sixel, from `[images] advertise_sixel`.
    /// **Off by default**: an application that sees the claim draws Sixel instead
    /// of its text preview -- chafa's grid of block elements, box-drawing
    /// strokes and coloured spaces, for yazi -- and the claim is the reader's
    /// to make rather than baud's to guess.
    pub sixel: baud_domain::SixelClaim,
}

/// The material, and the dimming over it.
///
/// **Every value here is one the crate's own example sweeps with a `GN_*`
/// environment variable** -- `examples/common/mod.rs` in
/// `macos-liquid-glass`, the content both `titled` and `borderless` draw,
/// and what was built as `glassterm` before the examples were renamed.
/// The defaults are that example's own calibration, which its header
/// records as "tuned against the system widget over one wallpaper rather
/// than derived from a measurement" -- so they are a starting point a
/// reader adjusts, which is why every one of them is a key.
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
    /// `style`: which material. `Clear` is the example's default and the
    /// one Apple's HIG pairs with a dimming layer over bright content,
    /// which is what a terminal body is.
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
    #[default]
    Clear,
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
    /// The example's own calibration, the values its `GN_*` knobs default
    /// to: `GN_PAPER_D` 0.22, `GN_PAPER_L` 0.12, `GN_GT_D` 0.54,
    /// `GN_GT_L` 0.14, `GN_HUE` 0.60, `GN_HSAT` 0.80, `GN_GLASS` clear,
    /// `GN_SHADOW` auto.
    pub fn defaults() -> Glass {
        Glass {
            style: Material::Clear,
            dim_dark: 0.22,
            dim_light: 0.12,
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

/// Which trail a cursor move leaves, from `[cursor] trail`: a fire rising
/// off kitty's quad -- the default, the owner's call, drawn as the quad on
/// the CPU path -- kitty's quad, whose four corners chase the cursor's,
/// sfte's sweep, a box swept from a tail chasing the cursor and fading out
/// toward it, baud's first trail, a one-cell ghost sweeping between the
/// cells, or none. `true` is the default and `false` is `off`, so a config
/// written when the key was a switch reads as it did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorTrail {
    Off,
    Ghost,
    Sweep,
    Quad,
    #[default]
    Fire,
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
/// driver. Only the macOS driver has a trail, so Linux never asks.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemMotion {
    Full,
    Reduced,
}

#[cfg(any(target_os = "macos", test))]
impl TrailPick {
    /// The trail drawn: a named one whatever the system says, and the
    /// default one -- the fire -- unless the system asked for reduced
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

/// When a blinking cursor stops blinking: never, the default, or after a
/// quiet time without input -- kitty's `cursor_stop_blinking_after 15.0`,
/// with a default of never here. Output never holds it off, so `tail -f`
/// stops blinking; it stops shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopAfter {
    Never,
    After(std::time::Duration),
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
            font_size: 13.0,
            padding: 8.0,
            colorspace: Space::Srgb,
            theme: ThemeChoice::Auto,
            floor: baud_render::Floor::DEFAULT,
            cursor_trail: TrailPick::Default,
            cursor_shape: CursorShape::Block,
            cursor_blink: BlinkChoice::Auto,
            cursor_stop: StopAfter::Never,
            screen_transition: ScreenTransition::Off,
            scroll: ScrollMode::Smooth,
            trail_scroll: TrailScroll::Ride,
            glass: Glass::defaults(),
            images: crate::host::IMAGE_BUDGET,
            sixel: baud_domain::SixelClaim::No,
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
            c.source = Some(path);
        }
        c
    }

    /// Reads `text` over `self`, and returns what it could not act on, one
    /// sentence each: a key no arm knows in the section it is in, and a
    /// value that fell back to a default. `load` prints them; a test reads
    /// them, because a config whose every value equals its default reads
    /// the same whether a line landed or was swallowed.
    fn apply(&mut self, text: &str) -> Vec<String> {
        let mut said = Vec::new();
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
                // decoded ARGB pixels, KiB of raw-Sixel scratch. Converted to
                // the store's own units (`ColorWord`s and bytes) here as a
                // local -- a `usize` field on `Config` would trip the audit,
                // but `ImageBudget` is a `baud-domain` type and exempt.
                ("images", "pixels_mb") => match value.trim().parse::<usize>() {
                    Ok(mb) => self.images.pixels = mb.saturating_mul(1 << 18),
                    Err(_) => warn_size(&value, "pixels_mb", &mut said),
                },
                ("images", "body_kb") => match value.trim().parse::<usize>() {
                    Ok(kb) => self.images.body = kb.saturating_mul(1 << 10),
                    Err(_) => warn_size(&value, "body_kb", &mut said),
                },
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
                        self.faces = faces;
                    }
                }
                ("glass", "style") => {
                    self.glass.style = match value.trim().trim_matches('"') {
                        "clear" => Material::Clear,
                        "regular" => Material::Regular,
                        other => {
                            said.push(format!(
                                "config: glass style {other:?} is not clear or regular; \
                                 using clear"
                            ));
                            Material::Clear
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

/// A `[glass]` fraction: parsed, and clamped to 0..=1 with a warning.
///
/// Clamped rather than rejected, for the reason a malformed `GN_ORIGIN` is
/// ignored rather than fatal in the example: a mistyped alpha should not
/// stop the window appearing. It says what it did, because a value
/// silently changed is the same defect as a key silently ignored.
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
            },
            None => FaceSpec {
                path: s.to_string(),
                index: 0,
            },
        }
    }
}

fn read_first() -> Option<(std::path::PathBuf, String)> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from);
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("BAUD_CONFIG") {
        paths.push(p.into());
    }
    if let Some(x) = xdg {
        paths.push(x.join("baud/config.toml"));
    }
    if let Some(h) = &home {
        paths.push(h.join(".config/baud/config.toml"));
    }
    paths.push("config.toml".into());
    paths
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
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

#[cfg(target_os = "macos")]
fn default_faces() -> Vec<FaceSpec> {
    DEFAULT_FACES.iter().map(|s| FaceSpec::parse(s)).collect()
}

/// Linux has no one system mono, so the default is the first of the usual
/// DejaVu Sans Mono locations that exists -- DejaVu carries the braille and
/// the box drawing itself -- and Noto Color Emoji beside it where it is. An
/// empty stack is reported at startup rather than guessed at; a config
/// names anything else.
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
    first(MONO).into_iter().chain(first(EMOJI)).collect()
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
        let specs: Vec<FaceSpec> = DEFAULT_FACES.iter().map(|s| FaceSpec::parse(s)).collect();
        let files: Vec<Vec<u8>> = specs
            .iter()
            .map(|f| std::fs::read(&f.path).unwrap_or_else(|e| panic!("{}: {e}", f.path)))
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

    /// The defaults are the crate example's own calibration, so a config
    /// that names no `[glass]` key at all gets what `glassterm` draws.
    /// Pinned by value: these are what the record quotes.
    #[test]
    fn the_defaults_are_the_examples_calibration() {
        let g = Config::defaults().glass;
        assert_eq!(g.style, Material::Clear);
        assert_eq!(g.shadow, Shadow::Auto);
        assert_eq!((g.dim_dark, g.dim_light), (0.22, 0.12));
        assert_eq!((g.tint_dark, g.tint_light), (0.54, 0.14));
        assert_eq!((g.hue, g.saturation), (0.60, 0.80));
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
            CursorTrail::Fire
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

    /// The cursor trail is the fire unless `[cursor] trail` names another,
    /// quoted or not; `true` and `false` read as they did when the key was a
    /// switch -- the default and none -- and a value that names no trail
    /// leaves the default with a warning rather than being read as one.
    #[test]
    fn the_cursor_trail_is_the_fire_unless_named() {
        assert_eq!(Config::defaults().cursor_trail, TrailPick::Default);
        assert_eq!(
            TrailPick::Default.effective(SystemMotion::Full),
            CursorTrail::Fire,
            "the default is the fire"
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

    /// `[images]` sets both knobs, in the reader's units: `pixels_mb` MiB of
    /// ARGB become `mb << 18` ColorWords, `body_kb` KiB become `kb << 10`
    /// bytes. Non-default values, so a section that lands as "the defaults
    /// anyway" cannot pass this. A whole number that is not one keeps the
    /// default rather than being read as zero, which a zero-size arena would
    /// refuse every image silently.
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

    #[test]
    fn an_images_section_sets_both_knobs() {
        assert_eq!(Config::defaults().images, crate::host::IMAGE_BUDGET);
        let c = read("[images]\npixels_mb = 8\nbody_kb = 512\n");
        assert_eq!(c.images.pixels, 8 << 18, "8 MiB of ColorWords");
        assert_eq!(c.images.body, 512 << 10, "512 KiB of scratch");
        // A non-number on one knob leaves that knob at the default and does
        // not disturb the other.
        let c = read("[images]\npixels_mb = plenty\nbody_kb = 512\n");
        assert_eq!(c.images.pixels, crate::host::IMAGE_BUDGET.pixels);
        assert_eq!(c.images.body, 512 << 10);
    }

    #[test]
    fn a_glass_section_sets_every_one_of_its_keys() {
        let c = read(
            r#"
[glass]
style      = "regular"
dim_dark   = 0.3
dim_light  = 0.05
tint_dark  = 0.4
tint_light = 0.2
hue        = 0.1
saturation = 0.9
shadow     = "never"
"#,
        );
        assert_eq!(c.glass.style, Material::Regular);
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
        let c = read("style = \"regular\"\nhue = 0.1\nshadow = \"never\"\n");
        assert_eq!(c.glass.style, Material::Clear, "top-level style leaked");
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
        assert_eq!(c.glass.style, Material::Clear);
        assert_eq!(c.glass.shadow, Shadow::Auto);
    }

    /// `background_alpha` is recognised only to be reported gone. The
    /// assertion with teeth is that it does not reach a dim: a reader's
    /// 255 must not turn into an opaque window through some later
    /// aliasing of the old key onto the new one.
    #[test]
    fn background_alpha_no_longer_reaches_a_dim() {
        let c = read("background_alpha = 255\nfont_size = 20\n");
        assert_eq!(c.glass.dim_dark, 0.22);
        assert_eq!(c.glass.dim_light, 0.12);
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
        // The stack it lists, not the built-in one it would fall back to.
        assert_eq!(c.faces.len(), 3, "the font stack was not read");
        assert!(
            c.faces[0].path.ends_with("Menlo.ttc"),
            "{}",
            c.faces[0].path
        );
        // The `[cursor]` section lands, and it sits after `fonts` so the
        // namespace does not swallow the stack -- which the face assert
        // above would catch.
        // The example's `trail` line is commented out -- naming a trail keeps
        // it under Reduce Motion -- so the section's landing is the shape and
        // blink below, which are named.
        assert_eq!(c.cursor_trail, TrailPick::Default);
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
                    !k.is_empty() && k.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
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
        assert_eq!(c.faces.len(), 2);
        assert_eq!(c.faces[1].path, "b");
        assert_eq!(c.faces[1].index, 2);
        assert_eq!(c.glass.dim_dark, 0.3);
    }
}
