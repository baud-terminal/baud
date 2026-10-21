use crate::*;

/// An OSC (Operating System Command) payload, resolved.
///
/// `[[source: ctlseqs]]` § "Set Text Parameters" gives the form as
/// `OSC Ps ; Pt BEL` or `OSC Ps ; Pt ST`, and both terminators are accepted:
/// "XTerm accepts either BEL or ST for terminating OSC sequences, and when
/// returning information, uses the same terminator used in a query."
///
/// Everything here is attacker-controlled: a window title is whatever the
/// remote end of an ssh session cared to send. Nothing in this type owns a
/// growable buffer, and the payload the core keeps is bounded and truncated
/// rather than rejected -- a title that is too long should be shortened, not
/// dropped, and must never be a reason to allocate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc<'a> {
    /// `OSC 0` — "Change Icon Name and Window Title to Pt."
    SetIconAndTitle(&'a [u8]),
    /// `OSC 1` — "Change Icon Name to Pt."
    SetIcon(&'a [u8]),
    /// `OSC 2` — "Change Window Title to Pt."
    SetTitle(&'a [u8]),
    /// `OSC 4 ; c ; spec` — "Change Color Number c to the color specified by
    /// spec." "Any number of c/spec pairs may be given", and each is applied
    /// in turn: [`ColorPairs`].
    SetColors(ColorPairs<'a>),
    /// `OSC 104 ; c` — reset the palette entries named, "Any number of c
    /// parameters", and with none "the entire table": [`ColorResets`].
    ResetColors(ColorResets<'a>),
    /// `OSC 5 ; c ; spec` -- "Change Special Color Number c to the color
    /// specified by spec", any number of pairs: [`ColorPairs`] whose items
    /// are [`ColorPair::Special`].
    SetSpecials(ColorPairs<'a>),
    /// `OSC 105 ; c` -- "Reset Special Color Number c", and with none "all
    /// special colors will be reset": [`ColorResets`].
    ResetSpecials(ColorResets<'a>),
    /// `OSC 6 ; c ; f` and `OSC 106 ; c ; f` -- "Enable/disable Special
    /// Color Number c": [`ColorModeSets`].
    ColorModes(ColorModeSets<'a>),
    /// `OSC 10 ; spec` to `OSC 19 ; spec` -- the dynamic colours.
    /// `[[source: ctlseqs]]`: "Change VT100 text foreground color to Pt",
    /// and `?` in place of a spec is a query "with a control sequence of the
    /// same form". "Each successive parameter changes the next color in the
    /// list. The value of Ps tells the starting point in the list":
    /// [`DynamicSpecs`].
    SetDynamic(DynamicSpecs<'a>),
    /// `OSC 110` / `OSC 111` -- reset the default foreground or background
    /// to what the embedder's theme says.
    ResetDynamic(DynamicColor),
    /// `OSC 8 ; params ; URI` — a hyperlink.
    ///
    /// An empty URI *closes* the current link rather than opening an empty one,
    /// which is how the [explicit hyperlink spec][osc8] ends a run.
    ///
    /// [osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
    Hyperlink { id: &'a [u8], uri: &'a [u8] },
    /// `OSC 133 ; <kind> ...` -- FinalTerm's semantic prompt marks, as
    /// every shell that ships integration emits them.
    ///
    /// **What baud wants them for is not what everyone else wants them
    /// for.** The usual uses are navigation and selection; here the
    /// load-bearing one is [`Semantic::Command`], because it is the
    /// boundary between the rows an application will redraw and the rows
    /// nobody will. A resize may re-cut the second and must not re-cut
    /// the first: a shell redraws its prompt by counting the rows it
    /// occupies and erasing that many, so changing the count behind its
    /// back leaves the top of the old prompt on screen. See
    /// `docs/research/notes/m-resize-and-reflow.md`.
    Semantic(Semantic<'a>),
    /// `OSC 60`, `OSC 61 ; cat`, `OSC 62 ; cat`: xterm's queries of the
    /// features it allows, [`AllowQuery`].
    Allowed(AllowQuery),
    /// `OSC 22 ; Pt`: the pointer's shape, [`PointerRequest`].
    Pointer(PointerRequest),
    /// `OSC 46 ; Pt` -- "Change Log File to Pt" (`[[source: ctlseqs]]`):
    /// [`LogFile`].
    LogFile(LogFile<'a>),
    /// `OSC 3 ; prop=value` -- "Set X property on top-level window. Pt
    /// should be in the form \"prop=value\", or just \"prop\" to delete
    /// the property" (`[[source: ctlseqs]]`): [`XProperty`].
    SetProperty(XProperty<'a>),
    /// `OSC 50 ; Pt`: "Set Font to Pt", or its query, [`FontRequest`].
    Font(FontRequest<'a>),
    /// Recognised as an OSC and not acted on.
    Unsupported,
}

/// What an `OSC 46` names: a file, or one of the two `Pt`s xterm takes for
/// none -- `?`, at which it rings the bell twice and changes nothing, and
/// an empty one, which its `do_osc` drops for want of data (`[[source:
/// xterm-misc]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogFile<'a> {
    Name(&'a [u8]),
    Query,
    Empty,
}

impl<'a> LogFile<'a> {
    pub fn parse(pt: &'a [u8]) -> LogFile<'a> {
        match pt {
            b"" => LogFile::Empty,
            b"?" => LogFile::Query,
            name => LogFile::Name(name),
        }
    }
}

/// What an `OSC 3` asks of a property: split at the first `=`, as
/// xterm's `ChangeXprop` splits it with `strchr` (`[[source: xterm-misc]]`),
/// a bare name deleting it. Only the properties with a counterpart here are
/// named ([`NamedProperty`]); every other one, an empty `Pt` among them, is
/// [`XProperty::Unmapped`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XProperty<'a> {
    Set(NamedProperty, &'a [u8]),
    Delete(NamedProperty),
    Unmapped,
}

impl<'a> XProperty<'a> {
    /// `Pt` of an `OSC 3`.
    pub fn parse(pt: &'a [u8]) -> XProperty<'a> {
        let (name, value) = match pt.iter().position(|&b| b == b'=') {
            Some(eq) => (&pt[..eq], Some(&pt[eq + 1..])),
            None => (pt, None),
        };
        match (NamedProperty::of(name), value) {
            (Some(p), Some(v)) => XProperty::Set(p, v),
            (Some(p), None) => XProperty::Delete(p),
            (None, _) => XProperty::Unmapped,
        }
    }
}

/// The X properties `OSC 3` maps: the window's name and its icon's, each
/// as ICCCM names it (`WM_NAME`, `WM_ICON_NAME`, typed `STRING` by xterm's
/// write) and as EWMH does (`_NET_WM_NAME`, `_NET_WM_ICON_NAME`, typed
/// `UTF8_STRING`), `[[source: icccm-2-0]]` chapter 4, "WM_NAME Property"
/// and "WM_ICON_NAME Property", and `[[source: ewmh-1-5-app]]` 5.1 and 5.3. Matched exactly, as an X atom's name is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NamedProperty {
    WmName,
    WmIconName,
    NetWmName,
    NetWmIconName,
}

impl NamedProperty {
    pub fn of(name: &[u8]) -> Option<NamedProperty> {
        match name {
            b"WM_NAME" => Some(NamedProperty::WmName),
            b"WM_ICON_NAME" => Some(NamedProperty::WmIconName),
            b"_NET_WM_NAME" => Some(NamedProperty::NetWmName),
            b"_NET_WM_ICON_NAME" => Some(NamedProperty::NetWmIconName),
            _ => None,
        }
    }

    /// Whether the property names the window or its icon.
    pub fn names(self) -> NamedLabel {
        match self {
            NamedProperty::WmName | NamedProperty::NetWmName => NamedLabel::Window,
            NamedProperty::WmIconName | NamedProperty::NetWmIconName => NamedLabel::Icon,
        }
    }

    /// The encoding the property's type implies: ICCCM's `STRING` is ISO
    /// 8859-1, EWMH's `UTF8_STRING` UTF-8.
    pub fn encoding(self) -> PropertyType {
        match self {
            NamedProperty::WmName | NamedProperty::WmIconName => PropertyType::String,
            NamedProperty::NetWmName | NamedProperty::NetWmIconName => PropertyType::Utf8String,
        }
    }
}

/// What a [`NamedProperty`] names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NamedLabel {
    Window,
    Icon,
}

/// The type a [`NamedProperty`] is written with, and so its encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyType {
    /// `STRING`: ISO 8859-1.
    String,
    /// `UTF8_STRING`: UTF-8, and a value that is not UTF-8 is none.
    Utf8String,
}

/// A pointer shape `OSC 22` names: one of the 77 of xterm's table,
/// "adapted from <X11/cursorfont.h>" (`LookupCursorShape`,
/// `[[source: xterm-misc]]` 889-975), matched by name with `strcmp`, case
/// kept. `[[source: ctlseqs]]`: "Change pointer cursor shape to Pt. The
/// parameter Pt sets the pointerShape resource."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerShape {
    /// "X_cursor"
    XCursor,
    /// "arrow"
    Arrow,
    /// "based_arrow_down"
    BasedArrowDown,
    /// "based_arrow_up"
    BasedArrowUp,
    /// "boat"
    Boat,
    /// "bogosity"
    Bogosity,
    /// "bottom_left_corner"
    BottomLeftCorner,
    /// "bottom_right_corner"
    BottomRightCorner,
    /// "bottom_side"
    BottomSide,
    /// "bottom_tee"
    BottomTee,
    /// "box_spiral"
    BoxSpiral,
    /// "center_ptr"
    CenterPtr,
    /// "circle"
    Circle,
    /// "clock"
    Clock,
    /// "coffee_mug"
    CoffeeMug,
    /// "cross"
    Cross,
    /// "cross_reverse"
    CrossReverse,
    /// "crosshair"
    Crosshair,
    /// "diamond_cross"
    DiamondCross,
    /// "dot"
    Dot,
    /// "dotbox"
    Dotbox,
    /// "double_arrow"
    DoubleArrow,
    /// "draft_large"
    DraftLarge,
    /// "draft_small"
    DraftSmall,
    /// "draped_box"
    DrapedBox,
    /// "exchange"
    Exchange,
    /// "fleur"
    Fleur,
    /// "gobbler"
    Gobbler,
    /// "gumby"
    Gumby,
    /// "hand1"
    Hand1,
    /// "hand2"
    Hand2,
    /// "heart"
    Heart,
    /// "icon"
    Icon,
    /// "iron_cross"
    IronCross,
    /// "left_ptr"
    LeftPtr,
    /// "left_side"
    LeftSide,
    /// "left_tee"
    LeftTee,
    /// "leftbutton"
    Leftbutton,
    /// "ll_angle"
    LlAngle,
    /// "lr_angle"
    LrAngle,
    /// "man"
    Man,
    /// "middlebutton"
    Middlebutton,
    /// "mouse"
    Mouse,
    /// "pencil"
    Pencil,
    /// "pirate"
    Pirate,
    /// "plus"
    Plus,
    /// "question_arrow"
    QuestionArrow,
    /// "right_ptr"
    RightPtr,
    /// "right_side"
    RightSide,
    /// "right_tee"
    RightTee,
    /// "rightbutton"
    Rightbutton,
    /// "rtl_logo"
    RtlLogo,
    /// "sailboat"
    Sailboat,
    /// "sb_down_arrow"
    SbDownArrow,
    /// "sb_h_double_arrow"
    SbHDoubleArrow,
    /// "sb_left_arrow"
    SbLeftArrow,
    /// "sb_right_arrow"
    SbRightArrow,
    /// "sb_up_arrow"
    SbUpArrow,
    /// "sb_v_double_arrow"
    SbVDoubleArrow,
    /// "shuttle"
    Shuttle,
    /// "sizing"
    Sizing,
    /// "spider"
    Spider,
    /// "spraycan"
    Spraycan,
    /// "star"
    Star,
    /// "target"
    Target,
    /// "tcross"
    Tcross,
    /// "top_left_arrow"
    TopLeftArrow,
    /// "top_left_corner"
    TopLeftCorner,
    /// "top_right_corner"
    TopRightCorner,
    /// "top_side"
    TopSide,
    /// "top_tee"
    TopTee,
    /// "trek"
    Trek,
    /// "ul_angle"
    UlAngle,
    /// "umbrella"
    Umbrella,
    /// "ur_angle"
    UrAngle,
    /// "watch"
    Watch,
    /// "xterm"
    Xterm,
}

impl PointerShape {
    /// Every shape, in xterm's table order.
    pub const ALL: [PointerShape; 77] = [
        PointerShape::XCursor,
        PointerShape::Arrow,
        PointerShape::BasedArrowDown,
        PointerShape::BasedArrowUp,
        PointerShape::Boat,
        PointerShape::Bogosity,
        PointerShape::BottomLeftCorner,
        PointerShape::BottomRightCorner,
        PointerShape::BottomSide,
        PointerShape::BottomTee,
        PointerShape::BoxSpiral,
        PointerShape::CenterPtr,
        PointerShape::Circle,
        PointerShape::Clock,
        PointerShape::CoffeeMug,
        PointerShape::Cross,
        PointerShape::CrossReverse,
        PointerShape::Crosshair,
        PointerShape::DiamondCross,
        PointerShape::Dot,
        PointerShape::Dotbox,
        PointerShape::DoubleArrow,
        PointerShape::DraftLarge,
        PointerShape::DraftSmall,
        PointerShape::DrapedBox,
        PointerShape::Exchange,
        PointerShape::Fleur,
        PointerShape::Gobbler,
        PointerShape::Gumby,
        PointerShape::Hand1,
        PointerShape::Hand2,
        PointerShape::Heart,
        PointerShape::Icon,
        PointerShape::IronCross,
        PointerShape::LeftPtr,
        PointerShape::LeftSide,
        PointerShape::LeftTee,
        PointerShape::Leftbutton,
        PointerShape::LlAngle,
        PointerShape::LrAngle,
        PointerShape::Man,
        PointerShape::Middlebutton,
        PointerShape::Mouse,
        PointerShape::Pencil,
        PointerShape::Pirate,
        PointerShape::Plus,
        PointerShape::QuestionArrow,
        PointerShape::RightPtr,
        PointerShape::RightSide,
        PointerShape::RightTee,
        PointerShape::Rightbutton,
        PointerShape::RtlLogo,
        PointerShape::Sailboat,
        PointerShape::SbDownArrow,
        PointerShape::SbHDoubleArrow,
        PointerShape::SbLeftArrow,
        PointerShape::SbRightArrow,
        PointerShape::SbUpArrow,
        PointerShape::SbVDoubleArrow,
        PointerShape::Shuttle,
        PointerShape::Sizing,
        PointerShape::Spider,
        PointerShape::Spraycan,
        PointerShape::Star,
        PointerShape::Target,
        PointerShape::Tcross,
        PointerShape::TopLeftArrow,
        PointerShape::TopLeftCorner,
        PointerShape::TopRightCorner,
        PointerShape::TopSide,
        PointerShape::TopTee,
        PointerShape::Trek,
        PointerShape::UlAngle,
        PointerShape::Umbrella,
        PointerShape::UrAngle,
        PointerShape::Watch,
        PointerShape::Xterm,
    ];

    /// The name xterm's table spells it with.
    pub fn name(self) -> &'static [u8] {
        match self {
            PointerShape::XCursor => b"X_cursor",
            PointerShape::Arrow => b"arrow",
            PointerShape::BasedArrowDown => b"based_arrow_down",
            PointerShape::BasedArrowUp => b"based_arrow_up",
            PointerShape::Boat => b"boat",
            PointerShape::Bogosity => b"bogosity",
            PointerShape::BottomLeftCorner => b"bottom_left_corner",
            PointerShape::BottomRightCorner => b"bottom_right_corner",
            PointerShape::BottomSide => b"bottom_side",
            PointerShape::BottomTee => b"bottom_tee",
            PointerShape::BoxSpiral => b"box_spiral",
            PointerShape::CenterPtr => b"center_ptr",
            PointerShape::Circle => b"circle",
            PointerShape::Clock => b"clock",
            PointerShape::CoffeeMug => b"coffee_mug",
            PointerShape::Cross => b"cross",
            PointerShape::CrossReverse => b"cross_reverse",
            PointerShape::Crosshair => b"crosshair",
            PointerShape::DiamondCross => b"diamond_cross",
            PointerShape::Dot => b"dot",
            PointerShape::Dotbox => b"dotbox",
            PointerShape::DoubleArrow => b"double_arrow",
            PointerShape::DraftLarge => b"draft_large",
            PointerShape::DraftSmall => b"draft_small",
            PointerShape::DrapedBox => b"draped_box",
            PointerShape::Exchange => b"exchange",
            PointerShape::Fleur => b"fleur",
            PointerShape::Gobbler => b"gobbler",
            PointerShape::Gumby => b"gumby",
            PointerShape::Hand1 => b"hand1",
            PointerShape::Hand2 => b"hand2",
            PointerShape::Heart => b"heart",
            PointerShape::Icon => b"icon",
            PointerShape::IronCross => b"iron_cross",
            PointerShape::LeftPtr => b"left_ptr",
            PointerShape::LeftSide => b"left_side",
            PointerShape::LeftTee => b"left_tee",
            PointerShape::Leftbutton => b"leftbutton",
            PointerShape::LlAngle => b"ll_angle",
            PointerShape::LrAngle => b"lr_angle",
            PointerShape::Man => b"man",
            PointerShape::Middlebutton => b"middlebutton",
            PointerShape::Mouse => b"mouse",
            PointerShape::Pencil => b"pencil",
            PointerShape::Pirate => b"pirate",
            PointerShape::Plus => b"plus",
            PointerShape::QuestionArrow => b"question_arrow",
            PointerShape::RightPtr => b"right_ptr",
            PointerShape::RightSide => b"right_side",
            PointerShape::RightTee => b"right_tee",
            PointerShape::Rightbutton => b"rightbutton",
            PointerShape::RtlLogo => b"rtl_logo",
            PointerShape::Sailboat => b"sailboat",
            PointerShape::SbDownArrow => b"sb_down_arrow",
            PointerShape::SbHDoubleArrow => b"sb_h_double_arrow",
            PointerShape::SbLeftArrow => b"sb_left_arrow",
            PointerShape::SbRightArrow => b"sb_right_arrow",
            PointerShape::SbUpArrow => b"sb_up_arrow",
            PointerShape::SbVDoubleArrow => b"sb_v_double_arrow",
            PointerShape::Shuttle => b"shuttle",
            PointerShape::Sizing => b"sizing",
            PointerShape::Spider => b"spider",
            PointerShape::Spraycan => b"spraycan",
            PointerShape::Star => b"star",
            PointerShape::Target => b"target",
            PointerShape::Tcross => b"tcross",
            PointerShape::TopLeftArrow => b"top_left_arrow",
            PointerShape::TopLeftCorner => b"top_left_corner",
            PointerShape::TopRightCorner => b"top_right_corner",
            PointerShape::TopSide => b"top_side",
            PointerShape::TopTee => b"top_tee",
            PointerShape::Trek => b"trek",
            PointerShape::UlAngle => b"ul_angle",
            PointerShape::Umbrella => b"umbrella",
            PointerShape::UrAngle => b"ur_angle",
            PointerShape::Watch => b"watch",
            PointerShape::Xterm => b"xterm",
        }
    }

    /// The shape a name spells, case kept, as `strcmp` compares.
    pub fn of(name: &[u8]) -> Option<PointerShape> {
        PointerShape::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// What an `OSC 22` asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerRequest {
    /// A shape of xterm's table.
    Shape(PointerShape),
    /// `Pt` empty: "If Pt is empty, or does not match any of the standard
    /// names, xterm uses the resource's default" (`[[source: ctlseqs]]`),
    /// which for baud is the embedder's own pointer. xterm's code differs:
    /// an empty `Pt` returns at "found no data" (misc.c 4136-4160) and
    /// changes nothing; the text is followed.
    Default,
    /// A name none of the 77 spells: the default too, and counted, a name
    /// baud could not honour.
    Unknown,
}

/// One of xterm's three feature queries (`[[source: ctlseqs]]`): "Query
/// allowed features (XTQALLOWED)", "Query disallowed features
/// (XTQDISALLOWED)" and "Query allowable features (XTQALLOWABLE)". Each
/// reply is `OSC Ps ; Pt` with a comma-separated list, the query's
/// terminator, and no `;` when the list is empty (`report_allowed_ops` and
/// `unparse_disallowed_ops`, `[[source: xterm-misc]]` 3939-3985 and
/// `[[source: xterm-reset]]`'s file 10666-10722).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllowQuery {
    /// `OSC 60`: the categories allowed.
    Allowed,
    /// `OSC 61 ; cat`: the subcategories of `cat` disallowed; `None` for a
    /// name that is no category, answered with an empty list.
    Disallowed(Option<AllowCategory>),
    /// `OSC 62 ; cat`: every subcategory of `cat`.
    Allowable(Option<AllowCategory>),
    /// `OSC 60` with data, or `OSC 61` or `62` without: xterm's `do_osc`
    /// returns at "found unwanted data" and "found no data" (misc.c
    /// 4100-4160) and answers nothing.
    Malformed,
}

/// The seven categories `OSC 60` names: xterm's resources, "zero or more of
/// these resource names" (`[[source: ctlseqs]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllowCategory {
    ColorOps,
    FontOps,
    MouseOps,
    PasteControls,
    TcapOps,
    TitleOps,
    WindowOps,
}

impl AllowCategory {
    /// In `OSC 60`'s order.
    pub const ALL: [AllowCategory; 7] = [
        AllowCategory::ColorOps,
        AllowCategory::FontOps,
        AllowCategory::MouseOps,
        AllowCategory::PasteControls,
        AllowCategory::TcapOps,
        AllowCategory::TitleOps,
        AllowCategory::WindowOps,
    ];

    /// The resource name, as `OSC 60` spells it.
    pub fn name(self) -> &'static [u8] {
        match self {
            AllowCategory::ColorOps => b"allowColorOps",
            AllowCategory::FontOps => b"allowFontOps",
            AllowCategory::MouseOps => b"allowMouseOps",
            AllowCategory::PasteControls => b"allowPasteControls",
            AllowCategory::TcapOps => b"allowTcapOps",
            AllowCategory::TitleOps => b"allowTitleOps",
            AllowCategory::WindowOps => b"allowWindowOps",
        }
    }

    /// The category an `OSC 61` or `62` names, case ignored, with xterm's
    /// alias `allowWinOps` for `allowWindowOps` (`x_strcasecmp` over
    /// `allow_categories`, charproc.c 10653-10661). `allowTitleOps` has no
    /// subcategories there, so it is no category for these two queries.
    pub fn of(name: &[u8]) -> Option<AllowCategory> {
        if name.eq_ignore_ascii_case(b"allowWinOps") {
            return Some(AllowCategory::WindowOps);
        }
        AllowCategory::ALL
            .into_iter()
            .filter(|&c| c != AllowCategory::TitleOps)
            .find(|c| name.eq_ignore_ascii_case(c.name()))
    }

    /// Its subcategories, in xterm's order: `tblColorOps` to
    /// `tblWindowOps` (`[[source: xterm-reset]]`'s file 10470-10620) as a
    /// default build compiles them, and `StatusLine`, which a default
    /// build leaves out with `OPT_STATUS_LINE` off (`[[source:
    /// xterm-ptyx-h]]` 800-801) and an `OPT_STATUS_LINE` build lists last
    /// (`[[source: xterm-reset]]`'s file, 10609-10610): baud has the status
    /// line.
    pub fn subcategories(self) -> &'static [&'static [u8]] {
        match self {
            AllowCategory::ColorOps => &[b"SetColor", b"GetColor", b"GetAnsiColor"],
            AllowCategory::FontOps => &[b"SetFont", b"GetFont"],
            AllowCategory::MouseOps => &[
                b"X10",
                b"Locator",
                b"VT200Click",
                b"VT200Hilite",
                b"AnyButton",
                b"AnyEvent",
                b"FocusEvent",
                b"Extended",
                b"SGR",
                b"URXVT",
                b"AlternateScroll",
            ],
            AllowCategory::PasteControls => &[
                b"NUL", b"SOH", b"STX", b"ETX", b"EOT", b"ENQ", b"ACK", b"BEL", b"BS", b"HT",
                b"LF", b"VT", b"FF", b"CR", b"SO", b"SI", b"DLE", b"DC1", b"DC2", b"DC3", b"DC4",
                b"NAK", b"SYN", b"ETB", b"CAN", b"EM", b"SUB", b"ESC", b"FS", b"GS", b"RS", b"US",
                b"NL", b"C0", b"DEL", b"STTY",
            ],
            AllowCategory::TcapOps => &[b"SetTcap", b"GetTcap"],
            AllowCategory::TitleOps => &[],
            AllowCategory::WindowOps => &[
                b"RestoreWin",
                b"MinimizeWin",
                b"SetWinPosition",
                b"SetWinSizePixels",
                b"RaiseWin",
                b"LowerWin",
                b"RefreshWin",
                b"SetWinSizeChars",
                b"MaximizeWin",
                b"FullscreenWin",
                b"GetWinState",
                b"GetWinPosition",
                b"GetWinSizePixels",
                b"GetWinSizeChars",
                b"GetScreenSizeChars",
                b"GetIconTitle",
                b"GetWinTitle",
                b"PushTitle",
                b"PopTitle",
                b"SetWinLines",
                b"ColumnMode",
                b"GetChecksum",
                b"SetChecksum",
                b"GetSelection",
                b"SetSelection",
                b"SetXprop",
                b"StatusLine",
            ],
        }
    }
}

/// One of xterm's five special colours, "Pc = 0 <- resource colorBD
/// (BOLD). Pc = 1 <- resource colorUL (UNDERLINE). Pc = 2 <- resource
/// colorBL (BLINK). Pc = 3 <- resource colorRV (REVERSE). Pc = 4 <- resource
/// colorIT (ITALIC)" (`[[source: ctlseqs]]`): the colour a cell with that
/// attribute is drawn in while its mode is on ([`ColorModes`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecialColor {
    Bold,
    Underline,
    Blink,
    Reverse,
    Italic,
}

impl SpecialColor {
    /// In `Pc`'s order.
    pub const ALL: [SpecialColor; 5] = [
        SpecialColor::Bold,
        SpecialColor::Underline,
        SpecialColor::Blink,
        SpecialColor::Reverse,
        SpecialColor::Italic,
    ];

    /// `Pc`: the number `OSC 5` names it by, and `OSC 4` by 256 more.
    pub fn code(self) -> u16 {
        match self {
            SpecialColor::Bold => 0,
            SpecialColor::Underline => 1,
            SpecialColor::Blink => 2,
            SpecialColor::Reverse => 3,
            SpecialColor::Italic => 4,
        }
    }

    pub fn of_code(c: u16) -> Option<SpecialColor> {
        SpecialColor::ALL.into_iter().find(|s| s.code() == c)
    }

    pub fn index(self) -> usize {
        usize::from(self.code())
    }

    /// The mode that turns it on.
    pub fn mode(self) -> ColorMode {
        match self {
            SpecialColor::Bold => ColorMode::Bold,
            SpecialColor::Underline => ColorMode::Underline,
            SpecialColor::Blink => ColorMode::Blink,
            SpecialColor::Reverse => ColorMode::Reverse,
            SpecialColor::Italic => ColorMode::Italic,
        }
    }
}

/// One of the six modes `OSC 6` and `OSC 106` turn on and off: "Pc = 0 <-
/// resource colorBDMode (BOLD). ... Pc = 4 <- resource colorITMode
/// (ITALIC). Pc = 5 <- resource colorAttrMode (Override ANSI)"
/// (`[[source: ctlseqs]]`). xterm's handler stops at 4 and ignores 5
/// ("unknown colorXXMode", `[[source: xterm-misc]]` 4197-4248); ctlseqs is
/// followed (Jev's lean, 0.66 and 0.74 after research).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    Bold,
    Underline,
    Blink,
    Reverse,
    Italic,
    /// colorAttrMode: "whether colorBD, colorBL, colorRV, and colorUL should
    /// override ANSI colors. If not, these are displayed only when no ANSI
    /// colors have been set for the corresponding position" (xterm(1)).
    Attr,
}

impl ColorMode {
    pub fn of_code(c: u16) -> Option<ColorMode> {
        match c {
            0 => Some(ColorMode::Bold),
            1 => Some(ColorMode::Underline),
            2 => Some(ColorMode::Blink),
            3 => Some(ColorMode::Reverse),
            4 => Some(ColorMode::Italic),
            5 => Some(ColorMode::Attr),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        1 << match self {
            ColorMode::Bold => 0,
            ColorMode::Underline => 1,
            ColorMode::Blink => 2,
            ColorMode::Reverse => 3,
            ColorMode::Italic => 4,
            ColorMode::Attr => 5,
        }
    }
}

/// Which of the six [`ColorMode`]s are on; all off at power-up, as xterm's
/// resources default to "false".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ColorModes(u8);

impl ColorModes {
    pub const NONE: ColorModes = ColorModes(0);

    pub fn has(self, m: ColorMode) -> bool {
        self.0 & m.bit() != 0
    }

    pub fn with(self, m: ColorMode, change: ModeChange) -> ColorModes {
        match change {
            ModeChange::Set => ColorModes(self.0 | m.bit()),
            ModeChange::Reset => ColorModes(self.0 & !m.bit()),
        }
    }

    /// **The rendition a cell draws with under these modes**, the one
    /// spelling of the rule, applied where a rendition is resolved for
    /// drawing, so turning a mode on or off recolours what is on the screen
    /// (Jev's lean, 0.88 and 0.90, over xterm's baking at `SGR` time: the
    /// manual's "should be displayed in color", and xterm's own `extract_fg`
    /// maps at draw time a cell whose colours are both default,
    /// `[[source: xterm-scroll]]` util.c 5680-5690).
    ///
    /// A cell whose foreground is default, or any cell under
    /// colorAttrMode, takes the first special colour whose mode is on and
    /// whose attribute it carries, blink before bold before underline
    /// before italic, as xterm's `MapToColorMode` orders them
    /// (`[[source: xterm-xterm-h]]`), and **the attribute is no longer
    /// drawn**: "displayed in color or as bold characters", the colour
    /// replacing it as xterm's `veryBoldColors` default of 0 has it (Jev,
    /// 1.00 both orders). A reversed cell whose background is default, or
    /// any under colorAttrMode, takes colorRV as its background and is not
    /// exchanged, so its text keeps its colour on a colorRV page, as xterm
    /// draws it: `setExtendedBG` gives it colorRV (`[[source:
    /// xterm-reset]]`'s file 1140-1150) and `ReverseOrHilite`, false for a
    /// reversed cell outside the selection while colorRVMode is on
    /// (`[[source: xterm-ptyx-h]]` 1603-1607), keeps `updatedXtermGC` from
    /// the exchange. A reversed cell with a background of its own keeps
    /// its exchange outside colorAttrMode, as the manual's "displayed only
    /// when no ANSI colors have been set" reads (`[[source: xterm-man]]`,
    /// colorAttrMode), where xterm's `ReverseOrHilite` drops it on every
    /// reversed cell (`[[source: xterm-ptyx-h]]` 1603-1607). `special` gives
    /// each colour's word, which the renderer gives as the text
    /// foreground's colour where none is set (Jev's lean, 0.32 and 0.28).
    pub fn apply(self, style: StyleWord, special: impl Fn(SpecialColor) -> ColorWord) -> StyleWord {
        if self == ColorModes::NONE {
            return style;
        }
        let attr = self.has(ColorMode::Attr);
        let a = style.attrs();
        let mut out = style;
        if attr || style.fg().is_default() {
            let took = [
                (SpecialColor::Blink, Attrs::BLINKS),
                (SpecialColor::Bold, Attrs::BOLD),
                (SpecialColor::Underline, Attrs::UNDERLINES),
                (SpecialColor::Italic, Attrs::ITALIC),
            ]
            .into_iter()
            .find(|&(c, family)| self.has(c.mode()) && a.bits() & family.bits() != 0);
            if let Some((c, family)) = took {
                out = out.with_fg(special(c)).with_attrs(a.without(family));
            }
        }
        if (attr || style.bg().is_default())
            && self.has(ColorMode::Reverse)
            && a.has(Attrs::REVERSE)
        {
            out = out
                .with_bg(special(SpecialColor::Reverse))
                .with_attrs(out.attrs().without(Attrs::REVERSE));
        }
        out
    }
}

/// One `c ; f` of an `OSC 6` or `OSC 106`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorModeSet {
    /// "enable the corresponding color mode if nonzero, disable it if zero".
    Mode(ColorMode, ModeChange),
    /// A `c` no mode has: ignored, as xterm's "unknown colorXXMode".
    Unknown,
    /// A field that is no number, or a `c` with no `f`, and the last item:
    /// xterm's walk breaks there (`[[source: xterm-misc]]` 4201-4224).
    Malformed,
}

/// The `c ; f` pairs of an `OSC 6` or `OSC 106`, walked as xterm's
/// `OSC_ColorMode` arm walks them: an empty field before a `;` is 0, a
/// field that is no number ends the walk, and so does a `c` with no `f`
/// after it. "If no parameters are given, this control has no effect"
/// (`[[source: ctlseqs]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorModeSets<'a> {
    rest: &'a [u8],
    walk: Walk,
}

impl ColorModeSets<'_> {
    /// One number at the head of `rest`, as xterm reads it: a `;` there is
    /// an empty field, 0, and is passed; else digits, and the `;` after
    /// them if there is one; anything else is no number.
    fn number(&mut self) -> Option<u16> {
        if let Some(r) = self.rest.strip_prefix(b";") {
            self.rest = r;
            return Some(0);
        }
        let n = self.rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        let v = self.rest[..n].iter().fold(0u16, |v, &b| {
            v.saturating_mul(10).saturating_add(u16::from(b - b'0'))
        });
        self.rest = &self.rest[n..];
        if let Some(r) = self.rest.strip_prefix(b";") {
            self.rest = r;
        }
        Some(v)
    }
}

impl Iterator for ColorModeSets<'_> {
    type Item = ColorModeSet;
    fn next(&mut self) -> Option<ColorModeSet> {
        if self.walk == Walk::Done || self.rest.is_empty() {
            return None;
        }
        let (Some(c), Some(f)) = (self.number(), self.number()) else {
            self.walk = Walk::Done;
            return Some(ColorModeSet::Malformed);
        };
        self.walk = Walk::Going;
        Some(match ColorMode::of_code(c) {
            Some(m) if f != 0 => ColorModeSet::Mode(m, ModeChange::Set),
            Some(m) => ColorModeSet::Mode(m, ModeChange::Reset),
            None => ColorModeSet::Unknown,
        })
    }
}

/// Which dynamic colour an `OSC 10`-family sequence names. Five of xterm's
/// ten: the text's two, the cursor's (`12`), and the selection's two, the
/// highlight background (`17`) and foreground (`19`), each drawn by the
/// renderer. The pointer's (`13`, `14`) and Tektronix's (`15`, `16`, `18`)
/// are out: the system draws the pointer, and baud has no Tektronix screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynamicColor {
    Foreground,
    Background,
    Cursor,
    /// `OSC 17`: "Change highlight background color" (`[[source:
    /// ctlseqs]]`), the page a selection is drawn on.
    HighlightBg,
    /// `OSC 19`: "Change highlight foreground color", the text a selection
    /// is drawn in.
    HighlightFg,
}

impl DynamicColor {
    /// How many there are: what a table indexed by [`DynamicColor::index`]
    /// holds.
    pub const COUNT: usize = 5;

    /// The colour an `OSC` number names, of the ten xterm numbers 10 to 19
    /// ([`DYNAMIC_FIRST`] to [`DYNAMIC_LAST`]); `None` for one baud keeps no
    /// colour for.
    pub fn of_code(code: u16) -> Option<DynamicColor> {
        match code {
            10 => Some(DynamicColor::Foreground),
            11 => Some(DynamicColor::Background),
            12 => Some(DynamicColor::Cursor),
            17 => Some(DynamicColor::HighlightBg),
            19 => Some(DynamicColor::HighlightFg),
            _ => None,
        }
    }

    /// The `Ps` that sets this colour; the reset is `100 + Ps`.
    pub fn code(self) -> u16 {
        match self {
            DynamicColor::Foreground => 10,
            DynamicColor::Background => 11,
            DynamicColor::Cursor => 12,
            DynamicColor::HighlightBg => 17,
            DynamicColor::HighlightFg => 19,
        }
    }
    pub fn index(self) -> usize {
        match self {
            DynamicColor::Foreground => 0,
            DynamicColor::Background => 1,
            DynamicColor::Cursor => 2,
            DynamicColor::HighlightBg => 3,
            DynamicColor::HighlightFg => 4,
        }
    }
}

/// Whether the screen is shown in reverse video: `?5`, DECSCNM -- and the
/// one spelling of what that does to the colours, which the renderer reads
/// to draw and the core reads to set, query and reset.
///
/// **Every cell draws with its foreground and background exchanged**, an
/// explicit `SGR` colour included: DEC STD 070 defines the mode as
/// reversing "the definitions of the foreground and background colors"
/// (`[[source: dec-std-070]]` § 5.6, page 5-94, PDF 349), and its Color Text
/// Extension says it "will affect which attribute bits from display memory
/// are used to render the text foreground and background color. It does not
/// affect the current rendition" (§ 5.6.3.2, page 5-107, PDF 362). The
/// owner decided this reading on 2026-10-13 over xterm's `ReverseVideo`,
/// which swaps the defaults and palette entries 0/7 and 8/15 and leaves
/// every other colour where it was (util.c 3269-3307, the file
/// `[[source: xterm-scroll]]` pins). A cell's own `SGR 7` exchanges again
/// ([`Video::drawn`]), and no palette entry moves, so an `OSC 4 ; i` or
/// `OSC 104 ; i` names the entry drawn wherever `i` is named.
///
/// A default cell drawn exchanged shows the stored background as its text
/// and the stored foreground as its page, so the colour shown in a role is
/// the other role's stored colour ([`Video::role`]): what an `OSC 10`,
/// `11`, `110` or `111` sent under `?5` acts on (note V, V17), and what the
/// margin, the scrollbar and a fade take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Video {
    #[default]
    Normal,
    Reverse,
}

impl Video {
    /// A cell's foreground and background as drawn under this video and
    /// the cell's own `attrs`: exchanged when exactly one of the screen's
    /// `?5` and the cell's `SGR 7` reverses them, so reverse inside reverse
    /// video draws the pair as stored. The one test of which side is the
    /// page: the colours, the contrast floor's words, the opaque-background
    /// test and the picture finder all take it.
    pub fn drawn<T>(self, attrs: Attrs, fg: T, bg: T) -> (T, T) {
        match (self, attrs.has(Attrs::REVERSE)) {
            (Video::Normal, false) | (Video::Reverse, true) => (fg, bg),
            (Video::Normal, true) | (Video::Reverse, false) => (bg, fg),
        }
    }

    /// The stored dynamic colour a role names under this video: the
    /// foreground and the background trade places, and the cursor colour is
    /// its own. Its own inverse.
    pub fn role(self, which: DynamicColor) -> DynamicColor {
        match (self, which) {
            (Video::Reverse, DynamicColor::Foreground) => DynamicColor::Background,
            (Video::Reverse, DynamicColor::Background) => DynamicColor::Foreground,
            (_, w) => w,
        }
    }

    /// The cursor colour after a change of video, given the text and the
    /// page shown *before* it: a colour equal to the text becomes the page,
    /// which the change then shows as the text. xterm's `ReverseVideo` does
    /// exactly that before its swap (`[[source: xterm-scroll]]` util.c
    /// 3293-3295), and `ReverseOldColors` to the copy `OSC 12 ; ?` answers
    /// from (`[[source: xterm-misc]]` misc.c 6418-6431), so the cursor keeps
    /// the text's colour instead of vanishing into the page. `OSC 12` is
    /// xterm's, so xterm's rule governs it under the exchange too, and its
    /// reason holds there: a default cell exchanged draws the old text
    /// colour as its page, which a cursor left in that colour would vanish
    /// into.
    ///
    /// A cursor colour nobody set ([`ColorWord::DEFAULT`]) is drawn in the
    /// text colour already, and stays unset; so does one whose page is not
    /// known, which is a core no embedder told its colours.
    pub fn cursor_across(cursor: ColorWord, text: ColorWord, page: ColorWord) -> ColorWord {
        if !cursor.is_default() && cursor == text && !page.is_default() {
            page
        } else {
            cursor
        }
    }
}

/// An X11 colour specification, as `XParseColor` accepts it.
///
/// `[[source: ctlseqs]]`: "The spec can be a name or RGB specification as per
/// XParseColor." XParseColor reads the `#` forms itself and sends anything
/// else Xcms does not take to the X server (`[[source: libx11-parsecol-b8d5a70]]`
/// 114-139), whose `OsLookupColor` compares with `strncasecmp` and an exact
/// length (`[[source: xserver-oscolor-98aed9d]]` 852-884): **case is ignored
/// and nothing else**, so `lime green` and `LimeGreen` are colours, each
/// spelled in the table, and `lime  green` is not. The table is X.Org's
/// rgb.txt (`[[source: xorg-rgb-0d2caec]]`), 782 names, generated into
/// `baud-domain/src/vt/x_colour_names.rs` and found by two hashes and one
/// comparison. `rgbi:` and the CIE and TekHVC forms are Xcms's, converted to
/// sRGB (`baud-domain/src/vt/xcms.rs`). What none of these reads -- a name
/// the server lacks, a malformed spec -- parses to [`XColor::Unresolved`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XColor<'a> {
    Rgb(Chan, Chan, Chan),
    /// `?` — a query. The reply must have the same shape as the setter.
    Query,
    Unresolved(&'a [u8]),
}

impl<'a> XColor<'a> {
    /// `rgb:R/G/B` with 1-4 hex digits per channel, `#RGB` / `#RRGGBB` /
    /// `#RRRGGGBBB` / `#RRRRGGGGBBBB`, `rgbi:`, `CIEXYZ:`, `CIEuvY:`,
    /// `CIExyY:`, `CIELab:`, `CIELuv:`, `TekHVC:`, or an X11 colour name.
    ///
    /// Each form is read to XParseColor's sixteen bits a channel first, as
    /// the Xlib manual's "Color Strings" defines them
    /// (`[[source: xlib-ch06-b8d5a70]]`): "Color strings are
    /// case-insensitive", so `RGB:FF/00/00` is red; `rgb:` digits are scaled,
    /// as Xcms scales them, truncating (`[[source: libx11-lrgb-b8d5a70]]`
    /// 1286-1375); and a `#` form's digits "represent the most significant
    /// bits of the value", so `#3a7` is `#3000a0007000`. Xcms lower-cases the
    /// spec before it matches a prefix (`[[source: libx11-cmscolnm-b8d5a70]]`
    /// 214-235). The channel kept is the value's high byte, as xterm computes
    /// it on TrueColor (`[[source: xterm-misc-9489b20]]` 2725-2733) and as the
    /// X resources reader draws it (decided there 2026-10-17); a 24-bit
    /// Xvfb gives the same pixel for every spec held in
    /// `[[source: xparsecolor-obs-rgb-out]]`: `#fff` is `f0f0f0`.
    pub fn parse(s: &'a [u8]) -> XColor<'a> {
        if s == b"?" {
            return XColor::Query;
        }
        let wide = match s.split_first() {
            Some((b'#', digits)) => sharp(digits),
            _ => match s.iter().position(|&b| b == b':') {
                Some(colon) if s[..colon].eq_ignore_ascii_case(b"rgb") => device(&s[colon + 1..]),
                Some(colon) => match super::xcms::Space::of(&s[..colon]) {
                    Some(space) => super::xcms::resolve(space, &s[colon + 1..]),
                    // A prefix no colour space has goes to the server as a
                    // name, as Xcms's refusal does, and no name holds a
                    // colon.
                    None => return XColor::named(s),
                },
                None => return XColor::named(s),
            },
        };
        match wide {
            Some([r, g, b]) => XColor::Rgb(high(r), high(g), high(b)),
            None => XColor::Unresolved(s),
        }
    }

    fn named(s: &'a [u8]) -> XColor<'a> {
        use super::x_colour_names::{POOL, SEEDS, SLOTS};
        match crate::colour_name::find(&SEEDS, &SLOTS, POOL, s) {
            Some([r, g, b]) => XColor::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
            None => XColor::Unresolved(s),
        }
    }
}

fn strip<'a>(s: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    (s.len() >= p.len() && &s[..p.len()] == p).then(|| &s[p.len()..])
}

/// The eight bits a 24-bit TrueColor visual keeps of a sixteen-bit value:
/// its high byte.
fn high(v: u16) -> Chan {
    Chan::from_u8(v.to_be_bytes()[0])
}

/// One to four hex digits, case ignored, and how many there were.
fn hex(s: &[u8]) -> Option<(u32, u32)> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        v = (v << 4) | char::from(b).to_digit(16)?;
    }
    Some((v, u32::try_from(s.len()).ok()? * 4))
}

/// `#` and three runs of equal width: each run is the high bits of its
/// channel, the rest zero.
fn sharp(digits: &[u8]) -> Option<[u16; 3]> {
    let w = digits.len() / 3;
    if !matches!(digits.len(), 3 | 6 | 9 | 12) {
        return None;
    }
    let mut out = [0u16; 3];
    for (c, run) in out.iter_mut().zip(digits.chunks(w)) {
        let (v, bits) = hex(run)?;
        *c = u16::try_from(v << (16 - bits)).ok()?;
    }
    Some(out)
}

/// `R/G/B` after `rgb:`, one to four digits each, scaled to sixteen bits as
/// Xcms scales them: `v * 0xffff / (2^bits - 1)`, truncated.
fn device(fields: &[u8]) -> Option<[u16; 3]> {
    let mut it = Split::new(fields, b'/');
    let mut out = [0u16; 3];
    for c in &mut out {
        let (v, bits) = hex(it.next()?)?;
        *c = u16::try_from(v * 0xffff / ((1 << bits) - 1)).ok()?;
    }
    it.next().is_none().then_some(out)
}

/// Split a byte slice on a separator without allocating.
struct Split<'a> {
    rest: Option<&'a [u8]>,
    sep: StreamByte,
}

impl<'a> Split<'a> {
    fn new(s: &'a [u8], sep: u8) -> Split<'a> {
        Split {
            rest: Some(s),
            sep: StreamByte::from_stream(sep),
        }
    }
}

impl<'a> Iterator for Split<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let cur = self.rest?;
        match cur
            .iter()
            .position(|&b| StreamByte::from_stream(b) == self.sep)
        {
            Some(i) => {
                self.rest = Some(&cur[i + 1..]);
                Some(&cur[..i])
            }
            None => {
                self.rest = None;
                Some(cur)
            }
        }
    }
}

/// xterm's `MAXCOLORS`: the 256 entries and the five special colours an
/// `OSC 4` names at 256 to 260 (`[[source: xterm-ptyx-h]]`).
const MAX_COLORS: u16 = 261;

/// A palette index as `OSC 4` and `OSC 104` name it: an entry, one of
/// xterm's special colours past the table, a number past both, or no
/// number at all.
enum Index {
    Entry(Chan),
    Special(SpecialColor),
    Past,
    Bad,
}

/// Where xterm's special colours begin in `OSC 4`'s numbering: "adding the
/// maximum number of colors (e.g., 88 or 256) to these codes"
/// (`[[source: ctlseqs]]`); `OSC 5` and `OSC 105` count from 0.
const SPECIAL_BASE: u16 = 256;

impl Index {
    /// Digits, however many: `0001` is 1, as xterm's `atoi` and `strtol`
    /// read it (`[[source: xterm-misc]]` 3001, 3064). A sign, a space or any
    /// other byte is no number, the grammar's `c` being one.
    fn of(s: &[u8], base: u16) -> Index {
        if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
            return Index::Bad;
        }
        let v = s
            .iter()
            .fold(0u16, |v, &b| {
                v.saturating_mul(10).saturating_add(u16::from(b - b'0'))
            })
            .saturating_add(base);
        match u8::try_from(v) {
            Ok(i) => Index::Entry(Chan::from_u8(i)),
            Err(_) => match SpecialColor::of_code(v.saturating_sub(SPECIAL_BASE)) {
                Some(c) if v < MAX_COLORS => Index::Special(c),
                _ => Index::Past,
            },
        }
    }
}

/// Where a walk over an `OSC` list is: before its first item, inside it, or
/// ended -- by its end, or by an item xterm stops at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Walk {
    Start,
    Going,
    Done,
}

/// One pair of an `OSC 4`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorPair<'a> {
    /// Entry `index` set to, or with [`XColor::Query`] asked for, `spec`.
    Entry { index: Chan, spec: XColor<'a> },
    /// One of xterm's special colours, `256 + c` in an `OSC 4` ("The
    /// special colors can also be set by adding the maximum number of colors
    /// ... to these codes in an OSC 4 control", `[[source: ctlseqs]]`), or
    /// `c` in an `OSC 5`, set to or asked for by `spec`.
    Special {
        which: SpecialColor,
        spec: XColor<'a>,
    },
    /// A pair off the grammar -- an index that is no number or lies past
    /// the special colours, an index with no spec, or no pair at all -- and
    /// the last item: xterm's walk will "quit on any error"
    /// (`[[source: xterm-misc]]` 3003).
    Malformed,
}

/// The pairs of an `OSC 4`, in order, walked as xterm's
/// `ChangeAnsiColorRequest` walks them (`[[source: xterm-misc]]`
/// 2993-3023): each `c ; spec` in turn, the walk ending after a malformed
/// pair or a spec no reader takes, the pairs before it applied. A `;` at
/// the end ends the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorPairs<'a> {
    rest: &'a [u8],
    walk: Walk,
    base: u16,
}

impl<'a> Iterator for ColorPairs<'a> {
    type Item = ColorPair<'a>;
    fn next(&mut self) -> Option<ColorPair<'a>> {
        if self.walk == Walk::Done || (self.walk == Walk::Going && self.rest.is_empty()) {
            return None;
        }
        self.walk = Walk::Done;
        let mut p = Split::new(self.rest, b';');
        let (Some(c), Some(spec)) = (p.next(), p.next()) else {
            return Some(ColorPair::Malformed);
        };
        let used = c.len() + 1 + spec.len();
        self.rest = self.rest.get(used + 1..).unwrap_or(&[]);
        let item = match Index::of(c, self.base) {
            Index::Entry(index) => {
                let spec = XColor::parse(spec);
                if !matches!(spec, XColor::Unresolved(_)) {
                    self.walk = Walk::Going;
                }
                ColorPair::Entry { index, spec }
            }
            Index::Special(which) => {
                let spec = XColor::parse(spec);
                if !matches!(spec, XColor::Unresolved(_)) {
                    self.walk = Walk::Going;
                }
                ColorPair::Special { which, spec }
            }
            Index::Past | Index::Bad => ColorPair::Malformed,
        };
        Some(item)
    }
}

/// The first and last of xterm's ten dynamic colour numbers: "The 10
/// colors (below) which may be set or queried using 1 0 through 1 9"
/// (`[[source: ctlseqs]]`).
pub const DYNAMIC_FIRST: u16 = 10;
pub const DYNAMIC_LAST: u16 = 19;

/// One spec of an `OSC 10`-family sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynamicSpec<'a> {
    /// The colour the spec's place in the list names, set to or, with
    /// [`XColor::Query`], asked for by `spec`.
    Color {
        which: DynamicColor,
        spec: XColor<'a>,
    },
    /// A request baud does not honour: a number it keeps no colour for (the
    /// pointer's 13 and 14, Tektronix's 15, 16 and 18), the specs past 19,
    /// counted once, or a sequence with no spec at all.
    Unheld,
}

/// The specs of an `OSC 10`-family sequence, each naming the next colour
/// from `Ps` on, as xterm's `ChangeColorsRequest` walks them
/// (`[[source: xterm-misc]]` 3680-3703): an empty field passes over its
/// colour, a spec no reader takes leaves its colour and the walk goes on,
/// and the walk ends at 19.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DynamicSpecs<'a> {
    code: u16,
    rest: &'a [u8],
    walk: Walk,
}

impl<'a> Iterator for DynamicSpecs<'a> {
    type Item = DynamicSpec<'a>;
    fn next(&mut self) -> Option<DynamicSpec<'a>> {
        loop {
            if self.walk == Walk::Done {
                return None;
            }
            if self.rest.is_empty() || self.code > DYNAMIC_LAST {
                let asked = self.rest.iter().any(|&b| b != b';');
                let none = self.walk == Walk::Start;
                self.walk = Walk::Done;
                return (asked || none).then_some(DynamicSpec::Unheld);
            }
            let field = Split::new(self.rest, b';').next().unwrap_or(b"");
            self.rest = self.rest.get(field.len() + 1..).unwrap_or(&[]);
            let code = self.code;
            self.code += 1;
            if field.is_empty() {
                continue;
            }
            self.walk = Walk::Going;
            return Some(match DynamicColor::of_code(code) {
                Some(which) => DynamicSpec::Color {
                    which,
                    spec: XColor::parse(field),
                },
                None => DynamicSpec::Unheld,
            });
        }
    }
}

/// One parameter of an `OSC 104`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorReset {
    Entry(Chan),
    /// One of xterm's special colours.
    Special(SpecialColor),
    /// A number past the special colours: ignored, as xterm's
    /// `ResetOneAnsiColor` ignores it (`[[source: xterm-misc]]` 3036-3046),
    /// and the walk goes on, so `104;1;999;2` resets 1 and 2.
    Past,
    /// No number, and the last item: xterm stops at "no number at all"
    /// and at "unexpected delimiter" (`[[source: xterm-misc]]` 3064-3069).
    Malformed,
}

/// The entries an `OSC 104` names, in order, as xterm's
/// `ResetAnsiColorRequest` reads them (`[[source: xterm-misc]]` 3050-3083).
/// [`ColorResets::all`] when it names none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorResets<'a> {
    rest: &'a [u8],
    walk: Walk,
    base: u16,
}

impl ColorResets<'_> {
    /// Whether the sequence named no entry: "If no parameters are given, the
    /// entire table will be reset" (`[[source: ctlseqs]]`).
    pub fn all(&self) -> bool {
        self.rest.is_empty() && self.walk == Walk::Start
    }
}

impl Iterator for ColorResets<'_> {
    type Item = ColorReset;
    fn next(&mut self) -> Option<ColorReset> {
        if self.walk == Walk::Done || self.rest.is_empty() {
            return None;
        }
        self.walk = Walk::Going;
        let c = Split::new(self.rest, b';').next().unwrap_or(b"");
        self.rest = self.rest.get(c.len() + 1..).unwrap_or(&[]);
        Some(match Index::of(c, self.base) {
            Index::Entry(i) => ColorReset::Entry(i),
            Index::Special(which) => ColorReset::Special(which),
            Index::Past => ColorReset::Past,
            Index::Bad => {
                self.walk = Walk::Done;
                ColorReset::Malformed
            }
        })
    }
}

impl<'a> Osc<'a> {
    /// Resolve an OSC payload -- the bytes between the introducer and the
    /// string terminator, with neither included.
    pub fn parse(payload: &'a [u8]) -> Osc<'a> {
        let mut it = Split::new(payload, b';');
        let Some(ps) = it.next() else {
            return Osc::Unsupported;
        };
        let rest = match payload.len() > ps.len() {
            true => &payload[ps.len() + 1..],
            false => &payload[payload.len()..],
        };
        match ps {
            b"0" => Osc::SetIconAndTitle(rest),
            b"1" => Osc::SetIcon(rest),
            b"2" => Osc::SetTitle(rest),
            b"4" => Osc::SetColors(ColorPairs {
                rest,
                walk: Walk::Start,
                base: 0,
            }),
            b"5" => Osc::SetSpecials(ColorPairs {
                rest,
                walk: Walk::Start,
                base: SPECIAL_BASE,
            }),
            b"105" => Osc::ResetSpecials(ColorResets {
                rest,
                walk: Walk::Start,
                base: SPECIAL_BASE,
            }),
            b"6" | b"106" => Osc::ColorModes(ColorModeSets {
                rest,
                walk: Walk::Start,
            }),
            b"10" | b"11" | b"12" | b"13" | b"14" | b"15" | b"16" | b"17" | b"18" | b"19" => {
                Osc::SetDynamic(DynamicSpecs {
                    code: ps.iter().fold(0, |v, &b| v * 10 + u16::from(b - b'0')),
                    rest,
                    walk: Walk::Start,
                })
            }
            b"110" => Osc::ResetDynamic(DynamicColor::Foreground),
            b"111" => Osc::ResetDynamic(DynamicColor::Background),
            b"112" => Osc::ResetDynamic(DynamicColor::Cursor),
            b"117" => Osc::ResetDynamic(DynamicColor::HighlightBg),
            b"119" => Osc::ResetDynamic(DynamicColor::HighlightFg),
            b"104" => Osc::ResetColors(ColorResets {
                rest,
                walk: Walk::Start,
                base: 0,
            }),
            b"8" => {
                // `OSC 8 ; params ; URI`. The params field carries `id=...`
                // and is allowed to be empty; the URI is everything after the
                // second semicolon, and may itself contain semicolons.
                let mut p = Split::new(rest, b';');
                let params = p.next().unwrap_or(b"");
                let uri = if rest.len() > params.len() {
                    &rest[params.len() + 1..]
                } else {
                    b""
                };
                Osc::Hyperlink {
                    id: link_id(params),
                    uri,
                }
            }
            b"133" => Osc::Semantic(Semantic::parse(rest)),
            b"3" => Osc::SetProperty(XProperty::parse(rest)),
            b"46" => Osc::LogFile(LogFile::parse(rest)),
            b"22" if rest.is_empty() => Osc::Pointer(PointerRequest::Default),
            b"22" => Osc::Pointer(match PointerShape::of(rest) {
                Some(shape) => PointerRequest::Shape(shape),
                None => PointerRequest::Unknown,
            }),
            b"50" => Osc::Font(FontRequest::parse(rest)),
            b"60" => Osc::Allowed(match payload.len() > ps.len() && !rest.is_empty() {
                true => AllowQuery::Malformed,
                false => AllowQuery::Allowed,
            }),
            b"61" | b"62" if rest.is_empty() => Osc::Allowed(AllowQuery::Malformed),
            b"61" => Osc::Allowed(AllowQuery::Disallowed(AllowCategory::of(rest))),
            b"62" => Osc::Allowed(AllowQuery::Allowable(AllowCategory::of(rest))),
            _ => Osc::Unsupported,
        }
    }
}

/// The `id=` value out of an `OSC 8` parameter field, if there is one.
///
/// Parameters are `key=value` pairs separated by `:`. Only `id` is defined by
/// the spec; anything else is ignored rather than guessed at.
fn link_id(params: &[u8]) -> &[u8] {
    let mut it = Split::new(params, b':');
    for kv in &mut it {
        if let Some(v) = strip(kv, b"id=") {
            return v;
        }
    }
    b""
}
