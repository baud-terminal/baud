//! **What a config read again changes in the running window** (`reload.rs`
//! hears the files, `config.rs` reads them): which settings differ, which
//! of those a running window takes at once and which wait for the next
//! launch, and the sentence the window shows for a file it could not use.
//!
//! **A setting a running window cannot take waits for the next launch**:
//! the image and clipboard areas and the printer the core was provisioned
//! with, the colour space the surfaces were made in, `vs16_width`, which
//! `DECRQM ?2027` reports as permanent, and the Sixel claim a program has
//! already read from `DA1`. `[text] ambiguous_width` changes at once: the
//! core measures the next character at it, and `DECRQM ?1021` answers set
//! or reset, never permanently. baud has one window a process
//! and no command that opens another, so "the next launch" is the only
//! new session there is. ghostty documents the same split ("Changing this
//! configuration at runtime will only affect new terminals"), and kitty
//! ("a handful of options cannot be dynamically changed and require a full
//! restart").
//!
//! Every field of [`Config`] is named in [`between`], with no `..`, so a
//! setting added later does not compile until it is given a [`Part`].

use crate::config::Config;

/// When a reloaded setting reaches the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Takes {
    Now,
    NextLaunch,
}

/// A group of settings a reload changes together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// `fonts`, `font_fallback`, and a font file named by path.
    Fonts,
    FontSize,
    Padding,
    /// `theme`, `[palette]`, its theme files, and `[glass]`.
    Palette,
    /// `[cursor] shape` and `blink`.
    CursorStyle,
    /// `[cursor] blink`, `blink_animation`, `blink_interval_ms`,
    /// `stop_blinking_after`.
    Caret,
    /// `[cursor] trail` and `trail_scroll`.
    Trail,
    /// `[screen] transition`.
    Transition,
    /// `[scroll] mode`.
    Scroll,
    /// `[screen] exit_status`.
    StatusMarks,
    /// `[cursor] unfocused_hollow`.
    Hollow,
    /// `[screen] badge`.
    Badge,
    /// `[screen] wrap_words`: the next frame lays the window out again.
    WrapWords,
    /// `[cursor] guide`.
    CursorGuide,
    /// `[images] animation`: the next turn steps or holds a kitty
    /// animation as it says.
    ImageAnimation,
    /// What the core is told: `[screen] checksum`, `substitute_mark`,
    /// `substitute_character`, `prompt_at_bottom`, `logging` and `print_*`,
    /// `[window] operations`, `[pointer] hide_while_typing`,
    /// `[font] changes` and `[clipboard] osc52`.
    TermOptions,
    /// `[keyboard]`: the Set-up modes, each whose value changed taking it
    /// in the running terminal now (`Term::change_set_up`).
    Keyboard,
    Bell,
    Emoji,
    /// `[screen] notifications`.
    Notifications,
    /// `[clipboard] copy_on_select`.
    CopyOnSelect,
    /// `[window] activity_lights`: the lights are put up or taken down at
    /// the next tick.
    ActivityLights,
    /// `[launch] splash`: off takes a splash still on screen down at once;
    /// on after launch shows nothing, the splash being the launch's.
    Splash,
    /// `config_reload` itself.
    Reload,
    /// `colorspace`.
    ColourSpace,
    /// `[glass] style`: the window's body is made once, at launch.
    GlassStyle,
    /// `[images] pixels_mb`.
    Images,
    /// `[images] advertise_sixel`.
    Sixel,
    /// `[text] ambiguous_width`: the next character's width.
    AmbiguousWidth,
    /// `vs16_width`.
    Vs16Width,
    /// `[screen] printing`.
    Printing,
    /// `[clipboard] body_kb`.
    ClipboardArea,
}

impl Part {
    pub fn takes(self) -> Takes {
        match self {
            Part::Fonts
            | Part::FontSize
            | Part::Padding
            | Part::Palette
            | Part::CursorStyle
            | Part::Caret
            | Part::Trail
            | Part::Transition
            | Part::Scroll
            | Part::StatusMarks
            | Part::Hollow
            | Part::Badge
            | Part::WrapWords
            | Part::CursorGuide
            | Part::ImageAnimation
            | Part::TermOptions
            | Part::Keyboard
            | Part::Bell
            | Part::Emoji
            | Part::Notifications
            | Part::CopyOnSelect
            | Part::ActivityLights
            | Part::Splash
            | Part::AmbiguousWidth
            | Part::Reload => Takes::Now,
            Part::ColourSpace
            | Part::GlassStyle
            | Part::Images
            | Part::Sixel
            | Part::Vs16Width
            | Part::Printing
            | Part::ClipboardArea => Takes::NextLaunch,
        }
    }

    /// The setting's name, as a sentence about it says it.
    pub fn name(self) -> &'static str {
        match self {
            Part::Fonts => "fonts",
            Part::FontSize => "font_size",
            Part::Padding => "padding",
            Part::Palette => "the palette",
            Part::CursorStyle => "[cursor] shape",
            Part::Caret => "the cursor's blink",
            Part::Trail => "[cursor] trail",
            Part::Transition => "[screen] transition",
            Part::Scroll => "[scroll] mode",
            Part::StatusMarks => "[screen] exit_status",
            Part::Hollow => "[cursor] unfocused_hollow",
            Part::Badge => "[screen] badge",
            Part::WrapWords => "[screen] wrap_words",
            Part::CursorGuide => "[cursor] guide",
            Part::ImageAnimation => "[images] animation",
            Part::TermOptions => "the terminal's options",
            Part::Keyboard => "[keyboard]",
            Part::Bell => "bell",
            Part::Emoji => "emoji",
            Part::Notifications => "[screen] notifications",
            Part::CopyOnSelect => "[clipboard] copy_on_select",
            Part::ActivityLights => "[window] activity_lights",
            Part::Splash => "[launch] splash",
            Part::Reload => "config_reload",
            Part::ColourSpace => "colorspace",
            Part::GlassStyle => "[glass] style",
            Part::Images => "[images] pixels_mb",
            Part::Sixel => "[images] advertise_sixel",
            Part::AmbiguousWidth => "[text] ambiguous_width",
            Part::Vs16Width => "vs16_width",
            Part::Printing => "[screen] printing",
            Part::ClipboardArea => "[clipboard] body_kb",
        }
    }
}

/// Whether the font files a config names by path read differently than
/// at the last reading, whatever the config says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceFiles {
    Same,
    Moved,
}

/// **The parts that differ between `old` and `new`**, each once, in
/// [`Part`]'s order. Both are configs as read, before their family names
/// are resolved, so a name compares as the name it is. The palette is
/// always in: a theme file named by the same path may have changed, and
/// rebuilding a palette is cheap.
pub fn between(old: &Config, new: &Config, faces: FaceFiles) -> Vec<Part> {
    let Config {
        source: _,
        faces: new_faces,
        stack,
        font_slots,
        font_fallback,
        font_size,
        padding,
        colorspace,
        theme: _,
        floor: _,
        palette_light: _,
        palette_dark: _,
        selection: _,
        cursor: _,
        cursor_trail,
        cursor_shape,
        cursor_blink,
        cursor_stop,
        blink_animation,
        blink_interval,
        screen_transition,
        exit_status,
        checksum,
        substitute,
        prompt_at_bottom,
        logging,
        printing,
        notifications,
        graphics_print,
        window_ops,
        activity_lights,
        launch_splash,
        pointer_mode,
        font_changes,
        scroll,
        trail_scroll,
        glass,
        images,
        sixel,
        image_animation,
        vs16_width,
        ambiguous_width,
        emoji,
        bell,
        clipboard,
        config_reload,
        unfocused_cursor,
        badge,
        wrap_words,
        cursor_guide,
        set_up,
    } = new;
    let same = |old: &[crate::config::FaceSpec], new: &[crate::config::FaceSpec]| {
        old.len() == new.len()
            && old.iter().zip(new).all(|(a, b)| {
                a.path == b.path
                    && a.index == b.index
                    && a.from == b.from
                    && a.role == b.role
                    && a.variations == b.variations
            })
    };
    let same_faces = same(&old.faces, new_faces) && same(&old.font_slots, font_slots);
    let mut out = Vec::new();
    let mut part = |differs: bool, p: Part| {
        if differs && !out.contains(&p) {
            out.push(p);
        }
    };
    part(
        !same_faces
            || old.stack != *stack
            || old.font_fallback != *font_fallback
            || faces == FaceFiles::Moved,
        Part::Fonts,
    );
    part(old.font_size != *font_size, Part::FontSize);
    part(old.padding != *padding, Part::Padding);
    part(true, Part::Palette);
    part(
        old.cursor_shape != *cursor_shape || old.cursor_blink != *cursor_blink,
        Part::CursorStyle,
    );
    part(
        old.cursor_blink != *cursor_blink
            || old.cursor_stop != *cursor_stop
            || old.blink_animation != *blink_animation
            || old.blink_interval != *blink_interval,
        Part::Caret,
    );
    part(
        old.cursor_trail != *cursor_trail || old.trail_scroll != *trail_scroll,
        Part::Trail,
    );
    part(
        old.screen_transition != *screen_transition,
        Part::Transition,
    );
    part(old.scroll != *scroll, Part::Scroll);
    part(old.exit_status != *exit_status, Part::StatusMarks);
    part(old.unfocused_cursor != *unfocused_cursor, Part::Hollow);
    part(old.badge != *badge, Part::Badge);
    part(old.wrap_words != *wrap_words, Part::WrapWords);
    part(old.cursor_guide != *cursor_guide, Part::CursorGuide);
    part(
        old.image_animation != *image_animation,
        Part::ImageAnimation,
    );
    part(
        old.checksum != *checksum
            || old.substitute != *substitute
            || old.prompt_at_bottom != *prompt_at_bottom
            || old.logging != *logging
            || old.graphics_print != *graphics_print
            || old.window_ops != *window_ops
            || old.pointer_mode != *pointer_mode
            || old.font_changes != *font_changes
            || old.clipboard.osc52 != clipboard.osc52,
        Part::TermOptions,
    );
    part(old.set_up != *set_up, Part::Keyboard);
    part(old.bell != *bell, Part::Bell);
    part(old.emoji != *emoji, Part::Emoji);
    part(old.notifications != *notifications, Part::Notifications);
    part(
        old.clipboard.copy_on_select != clipboard.copy_on_select,
        Part::CopyOnSelect,
    );
    part(
        old.activity_lights != *activity_lights,
        Part::ActivityLights,
    );
    part(old.launch_splash != *launch_splash, Part::Splash);
    part(old.config_reload != *config_reload, Part::Reload);
    part(old.colorspace != *colorspace, Part::ColourSpace);
    part(old.glass.style != glass.style, Part::GlassStyle);
    part(old.images != *images, Part::Images);
    part(old.sixel != *sixel, Part::Sixel);
    part(
        old.ambiguous_width != *ambiguous_width,
        Part::AmbiguousWidth,
    );
    part(old.vs16_width != *vs16_width, Part::Vs16Width);
    part(old.printing != *printing, Part::Printing);
    part(
        old.clipboard.budget != clipboard.budget,
        Part::ClipboardArea,
    );
    out
}

/// **What the core is told, from a config read again**: the settings both
/// drivers hand it at launch (`host.rs`), handed again. `abilities` is the
/// driver's window, as at launch.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn term_options(
    term: &mut baud_core::Term<'_>,
    c: &Config,
    abilities: baud_core::WindowAbilities,
) {
    crate::host::set_checksum(term, c.checksum);
    crate::host::set_substitute(term, c.substitute);
    crate::host::set_prompt_at_bottom(term, c.prompt_at_bottom);
    crate::host::set_window_ops(term, c.window_ops);
    crate::host::set_window_abilities(term, abilities, c.clipboard.osc52);
    crate::host::set_logging(term, c.logging);
    term.set_graphics_print(c.graphics_print);
    crate::host::set_pointer_mode(term, c.pointer_mode);
    crate::host::set_font_changes(term, c.font_changes);
}

/// **The one row the window shows about the config**: a file it could
/// not use, or what a file it used could not act on. Cleared by the next
/// reading that has nothing to say.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Notice(Option<String>);

impl Notice {
    /// Nothing to say.
    #[cfg(test)]
    pub const NONE: Notice = Notice(None);

    /// The first sentence of `said`, and how many more the log holds; none
    /// for an empty list.
    pub fn of(said: &[String]) -> Notice {
        Notice(said.first().map(|first| match said.len() {
            1 => first.clone(),
            n => format!("{first} (and {} more in the log)", n - 1),
        }))
    }

    /// A file the window could not use: `sentence` alone.
    pub fn broken(sentence: String) -> Notice {
        Notice(Some(sentence))
    }

    /// What the row says, if anything.
    #[cfg(test)]
    pub fn text(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// What the row says, as the renderer draws it
    /// (`Renderer::set_notice`): a space before, so the text stands off
    /// the edge as the link preview's bar does.
    pub fn codepoints(&self) -> Option<Vec<baud_domain::Codepoint>> {
        self.0.as_ref().map(|t| {
            core::iter::once(' ')
                .chain(t.chars())
                .map(baud_domain::Codepoint::from_char)
                .collect()
        })
    }

    /// The rows the notice takes from the window: one while it says
    /// anything (the grid gives a row to it, as alacritty's message bar
    /// takes rows).
    pub fn rows(&self) -> baud_domain::Rows {
        match self.0 {
            Some(_) => baud_domain::Rows::of_index(1),
            None => baud_domain::Rows::ZERO,
        }
    }
}

/// **The size a reload leaves the window at**: the new configured size,
/// unless the user zoomed away from the old one (`at` is not `base`), whose
/// zoom stays -- alacritty's "Do not update font size if it has been
/// changed at runtime" (`[[source: alacritty-0-15-1-window-context]]`).
/// Actual Size returns to `configured` either way, which the driver keeps
/// as its base.
pub fn size_after(at: f32, base: f32, configured: f32) -> f32 {
    match at == base {
        true => configured,
        false => at,
    }
}

/// **The sentence for a config the window took, the parts it could not
/// take in it named**: a setting that waits for the next launch is said
/// once, in the log, as the reload's own line.
pub fn later(parts: &[Part]) -> Option<String> {
    let names: Vec<&str> = parts
        .iter()
        .filter(|p| p.takes() == Takes::NextLaunch)
        .map(|p| p.name())
        .collect();
    match names.is_empty() {
        true => None,
        false => Some(format!(
            "config: {} {} effect at the next launch",
            names.join(", "),
            match names.len() {
                1 => "takes",
                _ => "take",
            }
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Config {
        let mut c = Config::defaults();
        let said = c.apply(text);
        assert!(said.is_empty(), "{said:?}");
        c
    }

    #[test]
    fn the_same_config_changes_only_the_palette() {
        let a = read("font_size = 13\n");
        assert_eq!(between(&a, &a, FaceFiles::Same), [Part::Palette]);
    }

    #[test]
    fn a_font_file_that_moved_is_a_font_change_with_the_config_unchanged() {
        let a = read("fonts = [\"/f/Mono.ttf\"]\n");
        assert_eq!(
            between(&a, &a, FaceFiles::Moved),
            [Part::Fonts, Part::Palette]
        );
    }

    /// **Each setting lands in its part**, and the parts a running window
    /// cannot take are the ones the guide names as waiting for the next
    /// launch.
    #[test]
    fn each_setting_is_the_part_it_names() {
        let base = Config::defaults();
        let cases: &[(&str, Part)] = &[
            ("fonts = [\"Menlo\"]\n", Part::Fonts),
            ("font_fallback = false\n", Part::Fonts),
            ("font_size = 20\n", Part::FontSize),
            ("padding = 2\n", Part::Padding),
            ("[cursor]\nshape = \"bar\"\n", Part::CursorStyle),
            ("[cursor]\nstop_blinking_after = 5\n", Part::Caret),
            ("[cursor]\nblink_animation = \"fade\"\n", Part::Caret),
            ("[cursor]\ntrail = \"plasma\"\n", Part::Trail),
            ("[cursor]\ntrail_scroll = \"stay\"\n", Part::Trail),
            ("[screen]\ntransition = true\n", Part::Transition),
            ("[scroll]\nmode = \"normal\"\n", Part::Scroll),
            ("[screen]\nexit_status = \"on\"\n", Part::StatusMarks),
            ("[cursor]\nunfocused_hollow = \"on\"\n", Part::Hollow),
            ("[screen]\nbadge = \"off\"\n", Part::Badge),
            ("[screen]\nwrap_words = \"off\"\n", Part::WrapWords),
            ("[cursor]\nguide = \"on\"\n", Part::CursorGuide),
            ("[images]\nanimation = \"off\"\n", Part::ImageAnimation),
            ("[screen]\nchecksum = \"off\"\n", Part::TermOptions),
            ("[screen]\nsubstitute_mark = \"on\"\n", Part::TermOptions),
            ("[screen]\nprompt_at_bottom = \"on\"\n", Part::TermOptions),
            (
                "[screen]\nsubstitute_character = \"#\"\n",
                Part::TermOptions,
            ),
            ("[screen]\nlogging = \"off\"\n", Part::TermOptions),
            ("[window]\noperations = \"off\"\n", Part::TermOptions),
            ("[font]\nchanges = \"off\"\n", Part::TermOptions),
            ("[clipboard]\nosc52 = \"off\"\n", Part::TermOptions),
            ("[keyboard]\nauto_repeat = \"off\"\n", Part::Keyboard),
            (
                "[keyboard]\napplication_cursor_keys = \"on\"\n",
                Part::Keyboard,
            ),
            ("[keyboard]\napplication_keypad = \"on\"\n", Part::Keyboard),
            ("[keyboard]\nnew_line = \"on\"\n", Part::Keyboard),
            ("bell = \"flash\"\n", Part::Bell),
            ("emoji = \"system\"\n", Part::Emoji),
            ("[screen]\nnotifications = \"off\"\n", Part::Notifications),
            (
                "[clipboard]\ncopy_on_select = \"off\"\n",
                Part::CopyOnSelect,
            ),
            (
                "[window]\nactivity_lights = \"off\"\n",
                Part::ActivityLights,
            ),
            ("[launch]\nsplash = \"off\"\n", Part::Splash),
            ("config_reload = \"off\"\n", Part::Reload),
            ("colorspace = \"display-p3\"\n", Part::ColourSpace),
            ("[glass]\nstyle = \"clear\"\n", Part::GlassStyle),
            ("[images]\npixels_mb = 10\n", Part::Images),
            ("[images]\nadvertise_sixel = true\n", Part::Sixel),
            ("vs16_width = \"one\"\n", Part::Vs16Width),
            ("[text]\nambiguous_width = \"wide\"\n", Part::AmbiguousWidth),
            ("[screen]\nprinting = \"off\"\n", Part::Printing),
            ("[clipboard]\nbody_kb = 8\n", Part::ClipboardArea),
        ];
        for (text, want) in cases {
            let got = between(&base, &read(text), FaceFiles::Same);
            let expected = match want {
                Part::Fonts | Part::FontSize | Part::Padding => vec![*want, Part::Palette],
                _ => vec![Part::Palette, *want],
            };
            assert_eq!(got, expected, "{text}");
        }
    }

    #[test]
    fn the_settings_a_running_window_cannot_take_wait_for_the_next_launch() {
        let later: Vec<Part> = [
            Part::ColourSpace,
            Part::GlassStyle,
            Part::Images,
            Part::Sixel,
            Part::Vs16Width,
            Part::Printing,
            Part::ClipboardArea,
        ]
        .into_iter()
        .filter(|p| p.takes() == Takes::NextLaunch)
        .collect();
        assert_eq!(later.len(), 7);
        assert_eq!(
            super::later(&[Part::FontSize, Part::Vs16Width, Part::Images]).as_deref(),
            Some("config: vs16_width, [images] pixels_mb take effect at the next launch")
        );
        assert_eq!(
            super::later(&[Part::FontSize, Part::Vs16Width]).as_deref(),
            Some("config: vs16_width takes effect at the next launch")
        );
        assert_eq!(super::later(&[Part::FontSize]), None);
        assert_eq!(super::later(&[Part::AmbiguousWidth]), None);
    }

    #[test]
    fn a_zoomed_size_stays_and_an_unzoomed_one_follows_the_config() {
        assert_eq!(size_after(13.0, 13.0, 16.0), 16.0, "not zoomed: follows");
        assert_eq!(size_after(18.0, 13.0, 16.0), 18.0, "zoomed: stays");
        assert_eq!(size_after(13.0, 13.0, 13.0), 13.0);
    }

    #[test]
    fn a_notice_says_the_first_sentence_and_counts_the_rest() {
        assert_eq!(Notice::of(&[]), Notice::NONE);
        assert_eq!(Notice::NONE.rows(), baud_domain::Rows::ZERO);
        let one = Notice::of(&["config: no key \"x\"; it is ignored".into()]);
        assert_eq!(one.text(), Some("config: no key \"x\"; it is ignored"));
        assert_eq!(one.rows(), baud_domain::Rows::of_index(1));
        let three = Notice::of(&["a".into(), "b".into(), "c".into()]);
        assert_eq!(three.text(), Some("a (and 2 more in the log)"));
    }
}
