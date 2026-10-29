# AJ. The launch splash: the colour logo, and the plasma it vanishes in

**Status: built 2026-10-24 (roadmap F1-19); the emerge built 2026-10-28
(section 9); its start and its turn 2026-10-29 (section 10); the sweep,
gone in half a second, 2026-10-29 (section 11). The questions as built:**

- **1. When.** Nothing of it until the window is shown (section 10). Under
  full motion, on the clock alone since 2026-10-28 (the lean): out of the
  plasma, then an even sweep out from 0.27 s, gone at 0.50 s, with one
  plasma and no second pass (section 11). Under reduced motion, as first built: on the
  shell's first visible cell or a 0.5 s cap from the window's being shown,
  whichever is first (E2 and a cap of 0.5 s; the cap is the lean);
  the vanish starts on the frame after that one.
- **2. Which plasma.** P1, against P2 with renders (from text on 2026-10-09). P4 was not offered: it needs a shader nothing in baud
  draws, and the plasma the trails use is baud's one plasma. P1 is built
  from the plasma trail's own model with no new shader: the box from the
  wordmark to the block collapses into the block on kitty's quad
  constants (chosen over a slower collapse).
- **3. Colour.** C1, the lockup's amber (chosen over C2, the cursor's
  colour that the 2026-09-25 rule gives every effect); the logo in the lockup's
  colours on the half the theme's polarity names.
- **4. Cells or pixels.** L2, against L1 with renders (from text on 2026-10-09): the lockup's geometry
  at 0.30 of the window's width, in the embedded
  JetBrains Mono Bold: weight 700 where the lockup asks 500, the one
  departure (chosen over the Regular).
- **5. Reduce Motion.** R2, the logo, then a 0.15 s fade, on macOS: the Linux driver reads no motion preference yet. No emerge: the logo whole from the first frame (section 9).
- **6. The key.** `[launch] splash = "on" | "off"`, on by default (settings
  take "on" and "off", 2026-10-19); the `[launch]` section is the lean.
- **7. Do not show again.** K, the key alone; nothing writes it.
- **8. Linux and the CPU path.** The same plasma: every path draws every
  effect from its Rust model now, so 5.8's premise no longer holds.

The research draft follows unchanged. It was revised on 2026-10-09 after a
read-only review; question 4's Medium face was weighed again on 2026-10-10,
and the splash's forks on 2026-10-24. The request, verbatim:

> "i'd like baud to start with the baud > [] logo drawn in the middle and then
> vanish with the plasma effect , the logo should be the one with the color, it
> should do that fast so it feels snappy... and a config option to turn it off
> (i believe hyprland has similar thing - do not show this again kinda thing)"

**Sources are cited by their manifest keys, in group `AJ`.**

- **Committed with the note.** Every tool and probe this note runs, and its
  saved output, is committed under `docs/research/sources/aj-observed/` as
  an `aj-obs-*` key:
  - the WCAG 2.3.1 area estimate;
  - the other-terminals probe, its wrapper and its output;
  - the font probe;
  - the logo and sfte searches.
- **Committed as fetched.** Apple's documentation JSON and the W3C pages
  are `retrieved`.
- **External.** Third-party source code and docs are `external`. Each
  entry carries the pinned URL, sha256 and byte count, and the bytes are
  not committed.
- **What stays untracked.** The fetched copies of the external sources,
  the sweep's ledger of them and the six terminals' clones were kept
  outside the repository. Neither is committed, and no claim rests on them beyond what
  the keyed, hashed sources carry.

Seven keys are already in `MANIFEST.toml` and are cited as they are:

- `wcag22`: a copy kept with the untracked evidence hashes to the committed
  file, `6e3c5fe3...`.
- `apple-improving-app-responsiveness`: today's fetch is byte-identical.
- `mlg-accessibility`: the window crate's Reduce Motion getter.
- `wezterm-termwindow-mod` and `wezterm-config-3725482`: wezterm files
  whose bytes match the sweep's copies.
- `sfte`.
- `wcag2ict-22`: defined by note AH's commit, from this note's manifest
  unchanged.

**In-repo citations are to baud at commit `473ff12`.** Line numbers are
that commit's. No file cited changes in the two later commits, which add
notes, their sources and a docs test.

Evidence tags:

- **Q**: quoted from the source named.
- **R**: read in code, not run.
- **C**: computed from the files.
- **M**: measured. Every M here is a figure already in a commit or
  `MEASUREMENTS.md`, cited by its commit or heading and scoped as the source
  scopes it. Nothing was measured for this note.
- **U**: a claim not verified.

## 0. In plain words

- **What the guidance says about a splash:**
  - **Launching.** Apple's HIG says "Launch instantly". It places a splash
    "as soon as launching completes" when there is no onboarding.
  - **Onboarding.** It asks that a splash be shown "just long enough for
    people to absorb the information at a glance".
  - **Motion.** "As much as possible, don't make people wait for an
    animation to complete before they can do anything".
  - **Timers.** Its Accessibility page asks apps to "Minimize use of
    time-boxed interface elements" and to "Prefer dismissing views with an
    explicit action".
  - **Reduce Motion.** The HIG asks apps to reduce automatic animations.
    It lists fades as one of five practices for doing so.
  - **What does not apply.** The HIG's launch-screen rules, "don't include
    logos" among them, are "Not applicable for macOS".
- **WCAG:**
  - **2.2.2.** Pause, Stop, Hide applies only past five seconds.
  - **2.3.3.** Animation from Interactions is AAA. WCAG2ICT gives no
    guidance on AAA for software, and is informative even for A and AA.
  - **2.3.1.** Three Flashes is the criterion with teeth, and its sources
    conflict on the area. The normative definition is 25% of a 10-degree
    field. With Note 1's estimate of that field (341 x 256 pixels at 1024
    x 768), the area is 21,824. The Understanding document's Intent puts
    "a flashing area of 87,296 CSS pixels", the whole field. The normative
    definition is stricter and governs.
  - **The shipped trail at 25% is not settled.** A model built from the
    code's constants (1.3) gives:
    - **Pass, with margin:** the knots, the filaments' sway and the ions,
      on area, at any plausible cell size, in linear or encoded blending.
    - **The body: depends on the font size and the blend space.**
      - **Retargets.** A retarget within `REIGNITE` keeps one ignition
        while the quad moves, so the body can flash more than once under
        it. Then the body has to pass on area, and on the model it does
        only through an opacity bound.
      - **Encoded blending.** baud blends in encoded sRGB space, where the
        bound is weaker: on WCAG's own curve the body passes up to about a
        33-41 pt cell.
    - **Red: a saturated red in either state.** That is the cursor colour
      (`OSC 12`) or the ground (an `SGR 41` cell, a dark-red background).
      A red flash has no luminance floor, so the opacity bound does not
      hold.
      - **A red cursor colour.** The body's 0.2 chromaticity step was not
        computed, so the body is counted at full reach as an upper bound.
        That bound fails on a diagonal from a 14 pt font.
      - **A red ground under the default cursor colour.** The parts that
        flash pass on area at the default font.

    It is not called safe here. Flashes counted on rendered frames would
    settle it, and the splash should not reuse the effect until they have
    been.
  - **What follows for the code.** `plasma.rs`'s "per 341x256 window"
    names the window the clause is evaluated over, and is consistent with
    both readings. "25% of" would remove the ambiguity.
  - **The logo.** Its native 300 x 130 box is 39,000. That is a box, not
    the ink, and the drawn size is Q4's choice. A logo-sized vanish has to
    be computed, and then counted on rendered frames, for the design
    chosen (U).
- **The time a splash can fill is already blank.** On this Mac:
  - the first frame is on screen about 88-92 ms after `main`;
  - the run loop, and with it the keyboard, starts after that. Its start
    is not marked, but the display link's first tick, which needs the
    loop, comes by about 133-139 ms in another run (**M**, 1.1). That
    leaves a window of up to about 45-52 ms in which a key's fate is
    unverified;
  - the shell's first bytes arrive at about 193-196 ms, through the
    development machine's login shell, whose rc takes about 150-170 ms
    (**M**, 3.6).

  The requirement is that a logo in that gap delay nothing. Whether a
  given design meets it is unmeasured until the acceptance test in
  section 7 runs. If the shell's first visible output *starts* the vanish,
  the logo holds for at least about 100 ms here: that is the gap to the
  first bytes, and the visible prompt's time is unmeasured. The vanish
  then adds its own length: 0.55-0.65 s for the plasma, 0.15 s for a fade
  on the transition's timing.
- **baud's plasma is a cursor trail**: a tube and three filaments from
  where the cursor was to where it is, ions spiralling off, in the cursor's
  colour, on the GPU only. It does not dissolve a logo as built. Section
  5.2 gives four ways to get to "the logo vanishes in plasma". One reuses
  the trail's shader and oracle: the logo's block is a cursor, so it can
  fly to the shell's cursor. That needs a new way to start a chase.
- **Hyprland is a loose analogy.**
  - **What it shows.** Its logo is a wallpaper that fades in and stays.
    Its "splash" is a line of random text at the bottom of the screen that
    also stays. Both are switched off by config keys that default to
    showing them.
  - **Its "don't show again" controls**, across its ecosystem:
    - a superseded update screen whose button points at a config key and
      writes nothing;
    - a welcome app whose "Finish" rewrites the user's config;
    - a welcome button that promises to "never show it again" and writes
      nothing.
  - **Its automatic state.** It also keeps a last-version file and a
    last-nag file in its data directory. These record what ran, whatever
    was shown, and are not a dismissal.
- **The closest precedent is Warp, not Hyprland.**
  - **First run.** Warp's GUI opens its first-run onboarding on a slide
    that draws the Warp logo, 64 x 64, centred above a shimmering "Welcome
    to Warp". The slide cannot be skipped in source.
  - **Every new session.** A zero-state block draws the Warp icon beside
    "New terminal session". It is on by default, shown only with AI
    enabled, and its "Don't show again" writes the user's `settings.toml`.
  - **The other five.** ghostty, kitty, iTerm2 and Windows Terminal draw
    no logo at launch, as far as the 26 search terms and the files read
    show (2.2). wezterm is the exception: when a newer release is cached,
    its update banner carries wezterm's app icon into each new pane.
  - **Where dismissals go:**
    - a separate state file: Windows Terminal's `state.json`, whose spec
      gives a reason;
    - Warp's private store, for some banners;
    - iTerm2's own settings store, its tips kept out of folder sync;
    - the user's config: Windows Terminal's close dialog, Warp's
      zero-state block;
    - nowhere, with a config key instead.
- **What is to decide** is in section 8: when the splash ends, which
  plasma, the colours, cells or pixels, its form under Reduce Motion, the
  key, whether "do not show again" means more than the key, and Linux.

## 1. What governs

### 1.1 Apple's Human Interface Guidelines (macOS)

The HIG pages were fetched as their documentation JSON on 2026-10-09. They
carry no version, only a change log, so each quote names the change log's
latest line.

**Launching** [[source: apple-hig-launching]], change log "June 10, 2024 --
Added guidance on displaying a splash screen":

> "Launching begins when someone opens your app or game, includes an initial
> download, and ends when the first screen is ready."

> "Launch instantly. People want to start interacting with your app or game
> right away, and sometimes they don't want to wait more than a couple of
> seconds."

> "If you need a splash screen, consider displaying it at the beginning of
> your onboarding flow. A splash screen is a beautiful graphic that
> succinctly communicates branding and other information you need to
> provide. If you don't provide an onboarding experience, you might display
> your splash screen as soon as launching completes."

**The page's Launch screens section does not apply on macOS.** It opens
"Not applicable for macOS, visionOS, or watchOS", and the page states
"macOS, visionOS, and watchOS don't require launch screens". Its rules are
not applied here. They include: "Don't advertise. The launch screen isn't a
branding opportunity. ... don't include logos or other branding elements
unless they're a fixed part of your app's first screen." They are quoted
only so that nobody cites them against a macOS splash. The HIG itself
separates the two things: "A launch screen isn't part of an onboarding
experience or a splash screen".

**When "launching completes" for baud.** The HIG says launching "ends when
the first screen is ready". baud has three candidates for that moment:

- **The first present**, about 88-92 ms after `main` (3.6). The frame is on
  the layer, but the menu, the key responder, the display link and the run
  loop are all set up after it. In `baud-app/src/app.rs` at `473ff12`:
  - the first frame, at 5106;
  - `install_main_menu`, at 5179;
  - `view.set_responder`, at 5181;
  - `view.start_display_link`, at 5182;
  - `ns_app.run()`, at 5185.

  Whether a key typed in that window is queued by AppKit or lost is
  unverified (U).
- **The run loop running.** From here keys reach the shell (3.7). Its
  start is not marked, but it is bounded:
  - **The first tick needs the loop.** The GPU's completion reaches the
    main thread through a call "which nothing delivers until
    `NSApplication::run` turns the loop, and AppKit's entry into `run()` is
    ~30 ms of its own" (measured with commit `fb85b76`).
  - **When the first tick comes.** "The Dock icon from the bundle", in
    MEASUREMENTS, puts it at 134.6 ms in the base arm, 139.4 in the kept
    change's arm and 133.3 in the A/A copy. Those are medians of six, on
    the M5 Max, through the login shell, from `.app` bundles. The same
    section says "The first present is before the first tick and is
    unmoved".

  So the loop starts by about 133-139 ms. That is up to about 45-52 ms
  after the 87.9 ms first present, which was measured in a different run,
  so the window in which a key's fate is unverified is at most that
  long.
- **The first frame with the shell's prompt.** The shell's first bytes
  arrive about 100 ms after the first present. The visible prompt's own
  time is unmeasured.

On the first reading, a logo on the first frame is "as soon as launching
completes". On the second and third, the first frame comes before the
moment, so the logo shows during launching, and the governing line is
Loading's "Show something as soon as possible" (below). The marks in
section 5 assume that no option delays any of the three moments. That is
the requirement, and it is unmeasured until section 7's acceptance test
runs.

**Onboarding** [[source: apple-hig-onboarding]], change log "June 10, 2024
-- Clarified different approaches to onboarding and added a guideline on
displaying a splash screen":

> "Briefly display a splash screen if necessary. If you need to include a
> splash screen, design a beautiful graphic that communicates succinctly.
> Aim to display your splash screen just long enough for people to absorb
> the information at a glance without feeling that it's delaying their
> experience."

> "If it makes sense to offer a separate tutorial, consider making it
> optional. If you let people skip the tutorial when they first launch your
> app or game, don't present it again on subsequent launches, but make sure
> it's easy for people to find if they want to view it later."

The second quote is about a *tutorial*, not a splash. It is cited only as
the HIG text nearest to "do not show again".

**Loading** [[source: apple-hig-loading]], change log "June 9, 2025 --
Revised guidance for storing downloads to reflect downloading large assets
in the background":

> "Show something as soon as possible. If you make people wait for loading
> to complete before displaying anything, they can interpret the lack of
> content as a problem with your app or game."

**Motion** [[source: apple-hig-motion]], change log "September 9, 2025 --
Added guidance for Liquid Glass":

> "Add motion purposefully, supporting the experience without overshadowing
> it. Don't add motion for the sake of adding motion. Gratuitous or
> excessive animation can distract people and may make them feel
> disconnected or physically uncomfortable."

> "Make motion optional. Not everyone can or wants to experience the motion
> in your app or game, so it's essential to avoid using it as the only way
> to communicate important information."

> "In apps, generally avoid adding motion to UI interactions that occur
> frequently."

> "Let people cancel motion. As much as possible, don't make people wait for
> an animation to complete before they can do anything, especially if they
> have to experience the animation more than once."

How far each line reaches:

- **"Make motion optional"** is scoped by its own sentence: "avoid using it
  as the only way to communicate important information". Whether a splash
  communicates "important information" is a reading. The HIG itself says a
  splash "succinctly communicates branding and other information".
- **"Let people cancel motion"** says "As much as possible, don't make
  people wait for an animation to complete before they can do anything,
  especially if they have to experience the animation more than once". A
  splash shown at every launch is experienced more than once. What the
  line supports is keys going to the shell while the splash runs. Whether
  a key also starts the vanish is E3, an option (5.1).

**Accessibility** [[source: apple-hig-accessibility]], change log "June 9,
2025 -- Added guidance and links for Assistive Access, Switch Control, and
Accessibility Nutrition Labels":

> "Minimize use of time-boxed interface elements. Views and controls that
> auto-dismiss on a timer can be problematic for people who need longer to
> process information, and for people who use assistive technologies that
> require more time to traverse the interface. Prefer dismissing views with
> an explicit action."

A splash that ends on a timer is a view that auto-dismisses on a timer, and
the HIG says a splash carries "branding and other information". The
paragraph is marked against each timer option in 5.1.

> "Be cautious with fast-moving and blinking animations. When you use these
> effects in excess, it can be distracting, cause dizziness, and in some
> cases even result in epileptic episodes. People who are prone to these
> effects can turn on the Reduce Motion accessibility setting. When this
> setting is active, ensure your app or game responds by reducing automatic
> and repetitive animations, including zooming, scaling, and peripheral
> motion. Other best practices for reducing motion include:"

The page then lists five items, quoted as written:

> - "Tightening animation springs to reduce bounce effects"
> - "Tracking animations directly with people's gestures"
> - "Avoiding animating depth changes in z-axis layers"
> - "Replacing transitions in x-, y-, and z-axes with fades to avoid motion"
> - "Avoiding animating into and out of blurs"

The requirement is "reducing automatic and repetitive animations". The fade
is one of five practices for meeting it, not the requirement itself.

**The setting's API.**
`NSWorkspace.accessibilityDisplayShouldReduceMotion`
[[source: apple-nsworkspace-reduce-motion]]:

> "A Boolean value that indicates whether the accessibility option to reduce
> motion is in an enabled state." "If this property's value is true, avoid
> large animations, especially those that simulate the third dimension. To
> receive updates when this setting changes, register for the
> accessibilityDisplayOptionsDidChangeNotification notification using
> notificationCenter."

The notification [[source: apple-nsworkspace-display-options-notification]]:
"To receive this notification, use notificationCenter to register for it. If
you use a different notification center to register, you won't receive the
notification." baud already reads the property through the window crate
([[source: mlg-accessibility]]) and polls it on the tick (3.4).

### 1.2 What "snappy" can be held to

No Apple text found gives a splash's duration. The HIG's measure is "at a
glance" (1.1). The figures that do exist are these:

- **One figure about launching.** Launching's "sometimes they don't want
  to wait more than a couple of seconds" (1.1).
- **[[source: apple-improving-app-responsiveness]]**, on input: "< 100 ms:
  Synchronous main thread work in response to a discrete user interaction",
  and "100 ms is the maximum delay for discrete user interaction before a
  delay becomes noticeable."
- **[[source: apple-understanding-hangs]]**, on input: "A delay of less
  than 100 ms in a discrete user interaction is rarely noticeable, but even
  a few hundred milliseconds can make people feel that an app is
  unresponsive", and "Most of Apple's developer tools start reporting
  issues when the period of unresponsiveness for the main run loop exceeds
  250 ms."
- **[[source: apple-reducing-launch-time]]**, on time after the first
  frame: "If your app still has to run code after it has drawn its first
  frame, but before the user can begin using the app, that time doesn't
  contribute to the launch-time metric. Extra startup activities still
  contribute to the user's perception of the app's responsiveness."

So the 100 ms bounds a *keystroke*, not the splash. A key pressed during
the splash must reach the shell as fast as one pressed after it. The last
quote is why the splash must not stand between the user and the shell.
**The splash's own duration has no source, and any figure for it is
unmeasured (U).** baud's own durations, which are not guidance, are in 3.3.
The total visible time of each option is in 5.1 and 5.2.

### 1.3 WCAG 2.2, and WCAG2ICT for software

WCAG is written for web content. [[source: wcag2ict-22]] (W3C Group Note, 11
December 2025) describes how its A and AA criteria *would* apply to non-web
software:

- **Its status.** "It provides informative guidance (guidance that is not
  normative and does not set requirements)."
- **Its scope.** It "does not seek to determine which WCAG 2 provisions
  (principles, guidelines, or success criteria) should or should not apply
  to non-web documents and software, but rather — if applied — how they
  would apply".
- **AAA.** It "does not provide guidance on applying AAA success criteria
  to non-web ICT, including the following definitions. ... motion
  animation".

Treating WCAG's A and AA criteria as requirements for baud, and AAA as
guidance, is this note's reading. WCAG2ICT does not impose it, and the
WCAG levels have not been ruled on. The rule on standards in general (2026-10-08; CONTRIBUTING.md, "What baud is built to"): "there is a standard we
need to follow and then there are algorithm optimization (including
hardware, l1-l3 caches, memory, etc) to make the standard run efficient,
that is what we are after". WCAG 2.2 itself advises against requiring AAA
wholesale: "It is not recommended that Level AAA conformance be required
as a general policy for entire sites because it is not possible to satisfy
all Level AAA success criteria for some content." ([[source: wcag22]],
§ 5.2.1, Note 2).

**2.2.2 Pause, Stop, Hide (Level A)** [[source: wcag22]]:

> "For any moving, blinking or scrolling information that (1) starts
> automatically, (2) lasts more than five seconds, and (3) is presented in
> parallel with other content, there is a mechanism for the user to pause,
> stop, or hide it unless the movement, blinking, or scrolling is part of an
> activity where it is essential"

WCAG2ICT: "This applies directly as written". **It does not reach a splash
that ends in under five seconds.** The Understanding document
[[source: wcag22-understanding-pause-stop-hide]] gives the reason for the
number: "Five seconds was chosen because it is long enough to get a user's
attention, but not so long that a user cannot wait out the distraction if
necessary to use the page." The criterion's Note 4 ("An animation that
occurs as part of a preload phase ... can be considered essential if
interaction cannot occur during that phase") does not fit baud:
interaction is meant to be possible during the splash.

**2.3.3 Animation from Interactions (Level AAA)** [[source: wcag22]]:

> "Motion animation triggered by interaction can be disabled, unless the
> animation is essential to the functionality or the information being
> conveyed."

The definition it rests on:

> "motion animation: addition of steps between conditions to create the
> illusion of movement or to give a sense of a smooth transition ... Motion
> animation does not include changes of color, blurring, or opacity which do
> not change the perceived size, shape, or position of the element."

WCAG2ICT gives no guidance on AAA, and on this note's reading (above) it is
guidance for baud, not a requirement. It is quoted because it draws the line the HIG draws: **an
opacity fade is not motion animation; filaments that drift and ions that
spiral are.** Its Understanding document
[[source: wcag22-understanding-animation-from-interactions]] names the
remedy baud already uses: "Take advantage of the reduce motion feature in
the user agent or operating system."

**2.3.1 Three Flashes or Below Threshold (Level A)** [[source: wcag22]]; its
Understanding document is [[source: wcag22-understanding-three-flashes]].

The criterion: "Web pages do not contain anything that flashes more than
three times in any one second period, or the flash is below the general
flash and red flash thresholds." WCAG2ICT: "This applies directly as
written".

The threshold, as WCAG 2.2 defines it:

> "a flash or rapidly changing image sequence is below the threshold (i.e.,
> content passes) if any of the following are true: there are no more than
> three general flashes and / or no more than three red flashes within any
> one-second period; or the combined area of flashes occurring concurrently
> occupies no more than a total of .006 steradians within any 10 degree
> visual field on the screen (25% of any 10 degree visual field on the
> screen) at typical viewing distance"

The threshold's Note 1, which estimates the field and is not the threshold
itself:

> "For general software or web content, using a 341 x 256 pixel rectangle
> anywhere on the displayed screen area when the content is viewed at 1024 x
> 768 pixels will provide a good estimate of a 10 degree visual field for
> standard screen sizes and viewing distances (e.g., 15-17 inch screen at
> 22-26 inches)."

A flash is "a pair of opposing changes in relative luminance", and a
general flash is such a pair "of 10% or more of the maximum relative
luminance (1.0) where the relative luminance of the darker image is below
0.80".

**The sources conflict on the area, and both are named here.** The
Understanding document's Intent
([[source: wcag22-understanding-three-flashes]]) reads Note 1's rectangle as
the whole allowance:

> "Substituting CSS pixels for the original pixel block means that the
> combined area of flashing becomes 341 x 256 CSS pixels, or a flashing area
> of 87,296 CSS pixels."

WCAG2ICT applies 2.3.1 "as described in Intent from Understanding Success
Criterion 2.3.1". It applies the threshold's definition, "general flash and
red flash thresholds", "directly as written and as described in the WCAG 2
glossary". The normative definition says "25% of any 10 degree visual
field", which at Note 1's estimate is 21,824 (**C**). The definition is the
stricter of the two, and it governs here.

`plasma.rs`'s module doc says: "WCAG 2.3.1's area clause is per 341x256
window, so a braid that stays inside the quad cannot exceed it whatever the
move's length" (`plasma.rs` 19-21). That names the window the clause is
evaluated over, and it is consistent with both readings. "25% of" would
remove the ambiguity. Its argument holds at 25% for a one-cell quad: about
7,249 at the default font, a 17 pt cell along a 426.4 pt diagonal (**C**).
The doc's other premise is its cursor-wide braid: "The braid's width is the
cursor's, across the axis" (17-18).

**The shipped trail at the normative 25%, a model (C).** The model and its
output:
[[source: aj-obs-flash-area-tool]]
[[source: aj-obs-flash-area]]

It reads no files. Its constants are copied from baud at `473ff12`:

- `REACH` = 1, `SHEATH` = 0.20, `DECAY` = 5, `FILAMENT` = 0.95 and the ions
  (`baud-render/src/plasma.rs`);
- the knot (0.72 ± 0.28), each filament's width (`max(PX_W(1 - k/2), 1.1
  px)`, `PX_W` ≤ 0.050 cell) and the gains (`PX_G` ≤ 1.0)
  (`shaders.metal` 283-284 and 312-315);
- the cell height, `round(1.32 em)` for JetBrains Mono
  (`baud-render/src/metrics.rs`);
- `REIGNITE` = 334 ms (`baud-app/src/trail.rs` 61-63).

How it measures:

- **The unit.** Points at 1x, taken as Note 1's pixels.
- **The window.** The longest straight run inside one 341 x 256 window is
  341 across or 426.4 on the diagonal. The model makes two simplifications
  that act in opposite directions. It does not clip a band to the window,
  which overstates the area. It takes a band's width as its height across
  the move, which on a diagonal understates the quad's footprint.
- **What a general flash needs.** A change of at least 0.10 in relative
  luminance, with the darker state below 0.80. WCAG 2.2's relative
  luminance is the piecewise sRGB curve (0.04045, exponent 2.4), and the
  model's sRGB row implements it. The least fill opacity that can make a
  flash is found by a fine search of a grey fill over a grey ground:
  - **in linear light:** 0.100, a white fill over black;
  - **in encoded space with an x^2.2 curve:** 0.0522, a black fill over a
    0.953 grey;
  - **in encoded space with WCAG 2.2's sRGB curve:** 0.0507, a black fill
    over a 0.955 grey.

  An earlier, coarser search gave 0.053 and 0.051. The shift is about 1%,
  and it changes no conclusion.
- **Which applies.** baud blends in encoded space. Every pipeline renders
  to `BGRA8Unorm`, not an `_sRGB` format (`baud-present-macos/src/metal.rs`
  524). The effects blend premultiplied OVER (528-532). The surface's bytes
  are sRGB (`app.rs` 682, `Space::Srgb`), and `shaders.metal` does no
  linearisation (**R**). So the encoded rows apply, and WCAG's own curve
  is the one quoted below.

**Results, figures from the model:**

- **The body's frequency holds only for moves without a retarget.**
  - **Without a retarget.** The tube and sheath brighten once at ignition
    and dim once, as `(1 - k)^2`. `REIGNITE` holds ignitions to "at most
    three times a second". That is the frequency clause's "no more than
    three".
  - **What a retarget does.** The body is drawn on the live quad: the
    shader measures `fx_distance` to the quad's current corners, and the
    renderer's `Vfx` holds the emitter as "kitty's quad this frame"
    (`effect.rs` 239-246). A move within `REIGNITE` of the last ignition
    retargets the quad and keeps the same ignition (`trail.rs` 32-35,
    279-287). So a region the quad leaves and re-enters can flash again
    under one ignition.
  - **So the body must also pass on area.** For a general flash, only
    pixels at or above the least flashing opacity count. The sheath stays
    at or above 0.0507 out to 0.275 cell, a 1.55-cell band. That passes
    while the cell is at most 41.3 pt across (fonts below 31.7 pt) or
    33.0 pt on the diagonal (fonts below 25.4 pt). With x^2.2 the bounds
    are 41.6 and 33.3 pt; in linear light, 50.1 and 40.1 pt.
- **The knots, the sway and the ions pass on area, with margin**, on WCAG's
  curve:
  - **A knot's swing** reaches 0.0507 within 1.53 filament widths: 4,315 at
    the default font along a full diagonal, passing to a 111 pt cell.
  - **A swaying filament's own opacity** stays at or above 0.0507 out to
    1.71 widths: 4,818 at the default font, passing to a 99.7 pt cell.
  - **In linear light** the knots reach 1.29 widths and the sway 1.50:
    3,638 and 4,223, passing to 132 and 114 pt.
  - **The ions.** 64 discs at their largest radius cover 804 at the
    default font, passing to a 116 pt cell.
- **Red flashes: a saturated red in either state.**
  - **The definition.** WCAG 2.2 defines a red flash as "any pair of
    opposing transitions involving a saturated red".
  - **The Understanding document's working definition.** A transition "to
    or from a state with a value R/(R + G + B) that is greater than or
    equal to 0.8", where "the difference between states is more than 0.2
    (unitless) in the CIE 1976 UCS chromaticity diagram".
  - **What can supply the red.** Either the cursor colour, which `OSC 12`
    can make a saturated red, or the ground: an `SGR 41` cell, or a
    dark-red palette background.
  - **A red cursor colour.** The 0.2 step was not computed for the body,
    so the body is counted at full reach as an upper bound. At full reach
    (the quad plus one cell either side, 3 cells) it passes while the cell
    is at most 21.3 pt across (fonts below 16.5 pt) or 17.1 pt on the
    diagonal (fonts below 13.3 pt). The default 13 pt font makes a 17 pt
    cell and 21,746 on the diagonal. That is a model figure 0.4% under the
    threshold, and the model's two simplifications pull in opposite
    directions. A 14 pt font (18 pt cell) gives 23,026, which is over. The
    knots count over the whole braid region, one cell high: 7,249 at the
    default font, passing to a 51 pt cell.
  - **A red ground under the default cursor colour.** The model computes
    the least opacity at which a light fill over a saturated-red ground
    crosses the 0.2 step, in encoded blending:
    - **over `SGR 41`'s `#CD0000`:** white crosses at 0.608, and the
      dark theme's default foreground (`0xD8` grey) at 0.645. Only the
      filament cores reach that (the body inside the quad is 0.28; a core
      reaches 0.28 + 0.72 x 0.95 = 0.964): 2,413 and 2,232 at the default
      font.
    - **over `#800000`:** white crosses at 0.487, still only in the cores:
      3,077.
    - **over a dark red, `#1A0000`:** white crosses at 0.131 and the
      default foreground at 0.150. There the tube (0.28) and the sheath out
      to 0.084 cell flash: 8,467 at the default font, passing to a 43.8 pt
      cell.

    These pass on area, but only a measurement that includes a red ground
    would see them.
  - **Whether a faint red knot crosses the 0.2 step over other grounds** was
    not computed (U).

**The result: the shipped trail's 2.3.1 safety is not settled.**

- **What passes with margin:** the knots, the sway and the ions, on area,
  in any of the three blend models.
- **What is not settled: the body.**
  - **Without a retarget,** it passes on frequency.
  - **Under retargets,** it passes only through the opacity bound, on
    WCAG's curve to about a 33-41 pt cell. So it depends on the font size.
  - **With a saturated red in either state,** the opacity bound does not
    hold. For a red cursor colour the full-reach upper bound fails on a
    diagonal from a 14 pt font. For a red ground under the default cursor
    colour, the parts that flash pass on area at the default font.
- **What would settle it.** A measurement: flashes counted on rendered
  frames, per 341 x 256 window, for a back-and-forth retarget, at the
  default and the largest fonts. The cases:
  - the cursor colour white, black and a saturated red;
  - a saturated-red ground (an `SGR 41` region, a dark-red palette
    background) under the default cursor colour.

  Until that is done the effect is not called safe, and the splash should
  not reuse it.
- **Measured, 2026-10-09** ([[source: aj-obs-flash-census]]). At the default 13 pt every effect
  passes every case; the plasma's worst flashing area is 8,274 pt², 38% of
  21,824. At 32 pt the plasma passes (20,071 pt²), and the fire, the default
  trail, and the orb fail on a light ground under back-and-forth retargets. At
  72 pt (`ZOOM_MAX`) the plasma fails 30 of 192 cases. So the plasma as built
  is not safe at every size, and a splash that reuses it inherits that; what
  to change is open.

**This is a finding about existing code, not an edit made here.**

**What follows for a splash:**

- **One appearance and one vanish** make at most one flash for the logo.
- **A plasma vanish (P1, P2)** inherits the body's open question above.
  Its knots and ions would pass on area on the same model, as long as the
  braid keeps the trail's filament widths.
- **The logo's own area.** Its native 300 x 130 box is 39,000, but that is
  a box, not the ink, and the drawn size is Q4's choice. It matters only
  if the logo itself changed more than three times a second, which no
  option does.
- **The plasma's per-pixel transition rate** has not been worked out:
  - the knots pass a point at `KNOT` = 2.5 a second, the three a third of
    a cycle apart (`shaders.metal` 314);
  - the filaments sway at 0.80-1.35 cycles a second, with a counter-flowing
    harmonic at 0.85-1.20 (`PX_F`, `PX_F2`, 279-281, used at 310-311).

  It would matter only where a part fails on area (U).

### 1.4 What no text says

How long a splash lasts, beyond "at a glance" and the time-boxed guidance's
preference for an explicit dismissal. Whether a splash shows at every launch
or only the first. What the off switch is called. Whether a "don't show
again" writes the user's configuration. The nearest texts are the tutorial
sentence and the time-boxed paragraph in 1.1.

## 2. Prior art (practice, not standard)

### 2.1 Hyprland, at source

The pins:

- Hyprland `663b29b846e4bad17cfa29941f6d2fbb6b61f139` (2026-10-09);
- hyprland-guiutils `4c30cf3097ea963c0e250749ee0c59f8b08816d6` (2026-08-11);
- hyprland-qtutils `56d993aced40a9058350d24ff4a7fb4eb961f89b` (2025-11-06),
  superseded: "This repo has been superseded by hyprland-guiutils"
  ([[source: hyprland-qtutils-readme]]);
- hyprland-welcome `6c8495a59832008c7ba3e085d86dbc47f6ffe8cb` (2025-11-20),
  "Archived, as this has been moved into guiutils"
  ([[source: hyprland-welcome-archived-readme]]);
- the wiki, `c3f93fd8acf083d7ce27be78061b69784ab6fd32` (2026-10-09).

All five are BSD-3-Clause at these pins. Hyprland's config is Lua at this
commit (`hl.config({...})`).

**The splash is text, and it stays** [[source: hyprland-compositor]],
[[source: hyprland-renderer]].

- **What it is.** At startup `setRandomSplash()` picks one line from a
  list, with seasonal lists for 23-27 December and New Year
  (`src/Compositor.cpp` 245-261, called at 236; the lists in
  `src/helpers/Splashes.hpp`, [[source: hyprland-splashes]]).
- **Where it is drawn.** `renderBackground` draws it on every frame of the
  background, centred horizontally, with its bottom at 98% of the
  monitor's height, in a font sized `monitorSize.y / 76`
  (`src/render/Renderer.cpp` 1467-1478; Pango through Cairo, 3560-3600).
- **Its colour.** `misc:col.splash`, default `0x55ffffff`.
- **How it ends.** It has no animation and no end (**R**).

**The logo is the wallpaper, and it fades in.** With
`misc:disable_hyprland_logo` false, the background is one of the default
wallpapers, "the random Hyprland logo / anime girl background". Its opacity
is warped to 0 and animated to 1 when the texture is ready (`Renderer.cpp`
1420-1421, read at 1461):

```
    pMonitor->m_backgroundOpacity->setValueAndWarp(0.F);
    *pMonitor->m_backgroundOpacity = 1.F;
```

It does not vanish. A wallpaper client can cover it:
`renderMonitorBackground` draws the background, then the background
layer's surfaces over it (`Renderer.cpp` 1151-1155). The wiki notes that
"hyprpaper has its own setting to disable" the splash. (**R**.)

**The keys and their defaults** (`src/config/values/ConfigValues.cpp`,
[[source: hyprland-config-values]]; the wiki,
[[source: hyprland-wiki-config-options]]). Each `disable_*` key defaults
to `false`, so the defaults show the logo and the splash.

| key | default | its text in the code | the wiki's text (`config-options.md`) |
|---|---|---|---|
| `misc:disable_hyprland_logo` (561) | `false` | "disables the random Hyprland logo / anime girl background. :(" | (615) "Disables the random Hyprland logo/anime girl background. :(" |
| `misc:disable_splash_rendering` (562) | `false` | "disables the Hyprland splash rendering." | (617) "Disables the Hyprland splash rendering (the small random bit of text on the background). Requires a monitor reload to take effect. Note hyprpaper has its own setting to disable it" |
| `misc:col.splash` (563) | `0x55ffffff` | "Changes the color of the splash text." | |
| `misc:splash_font_family` (565) | empty, falls back to `misc:font_family` | "Changes the font used to render the splash text." | |
| `misc:force_default_wallpaper` (566) | `-1` | "Force any of the 3 default wallpapers. [-1/0/1/2]" | |
| `ecosystem:no_update_news` (715) | `false` | "disable the popup that shows up when you update hyprland to a new version." | (769) |
| `ecosystem:no_donation_nag` (716) | `false` | "disable the popup that shows up twice a year encouraging to donate." | (768) |

The generated example config writes the logo key out with its default and a
comment, so a new user meets the switch: `disable_hyprland_logo   = false,
-- If true disables the random hyprland logo / anime girl background. :(`
(`example/hyprland.lua` 209, [[source: hyprland-example-config]]).

**"Do not show this again" in Hyprland's ecosystem: three controls, none for
the logo or the splash, and automatic state in two files.**

1. **A button that points at a config key and writes nothing, in the
   superseded app only.**
   - **The qtutils button.** The Qt update screen has a button "Don't show
     this when I update" (`hyprland-qtutils/utils/update-screen/main.qml`
     94, [[source: hyprland-qtutils-update-screen-qml]]). It opens a dialog
     reading "If you wish to disable this dialog, set
     ecosystem:no_update_news to true in your Hyprland config." (`main.cpp`
     54, [[source: hyprland-qtutils-update-screen]]).
   - **Its guiutils successor has no such button.** Its buttons are
     Support, Open news and Thanks
     (`hyprland-guiutils/utils/update-screen/src/main.cpp` 130-153). Its
     text says "You can turn off this screen in your Hyprland config."
     (40, [[source: hyprland-guiutils-update-screen]]) (**R**).
2. **A welcome app that rewrites the user's config.**
   - **When it runs.** Hyprland launches `hyprland-welcome` only while the
     config carries the generated `autogenerated` flag
     (`src/managers/WelcomeManager.cpp` 11-33,
     [[source: hyprland-welcome-manager]]).
   - **The flag.** The generated config opens with a four-line banner and
     then carries `hl.config({ autogenerated = true }) -- remove this line
     to remove the warning` (`src/config/lua/DefaultConfig.hpp` 6-9 and
     11, [[source: hyprland-default-config]]).
   - **"Finish".** The welcome app's "Finish" calls `removeAutogen()`. That
     reads `hyprland.lua`, deletes the line and writes the whole file back
     with `std::ofstream ofs(*PATH, std::ios::trunc)`
     (`hyprland-guiutils/utils/welcome/src/main.cpp` 480-500, called at
     798; [[source: hyprland-guiutils-welcome]]).
   - **The default-app pickers** rewrite the file too
     (`updateDefaultConfigVar`, 449-478, called at 631) (**R**).
3. **A button whose text promises more than its code does.**
   - **The promise.** The welcome app's first page says: "you can click
     the "Thanks, but I don't need help" button on the bottom. It will
     close this window and never show it again." (`main.cpp` 55).
   - **The handler.** It closes the window, destroys the backend, and does
     nothing else (774-781). It does not call `removeAutogen()`, so the
     flag stays and `WelcomeManager` would launch the app again at the
     next start (**R**, not run).
   - **The archived predecessor** writes nothing from either button
     (`hyprland-welcome/src/main.cpp` 600-628,
     [[source: hyprland-welcome-archived]]).

   This is Hyprland's departure from its own text, recorded as Hyprland's.
4. **Automatic state, not a dismissal.** Two files live under
   `NFsUtils::getDataHome()`: `$XDG_DATA_HOME/hyprland/`, or
   `$HOME/.local/share/hyprland/` (`src/helpers/fs/FsUtils.cpp` 12-35; the
   directory is created at 39 and given owner-only permissions at 44;
   [[source: hyprland-fsutils]]).
   - **`lastVersion`** records the last x.y version that ran, or `0.0.0`
     after the first run.
     - **On first run** it is written `0.0.0`, with nothing shown
       (`VersionKeeperManager.cpp` 30-35,
       [[source: hyprland-version-keeper]]).
     - **On a new x.y release** (Hyprland's "major") it is written
       `HYPRLAND_VERSION` at 43. That is *before* the `no_update_news`
       check (45) and the executable check (50), so it is written whether
       or not the screen is shown. Same-version and patch launches return
       at 38-41 without writing.
   - **`lastNag`** records the first-run time and the last nag's time.
     - **On first run** it is written with no nag shown
       (`DonationNagManager.cpp` 62-66, [[source: hyprland-donation-nag]]).
     - **After a nag** it is written again: after a date nag at 98-100,
       after a version nag at 113-115. Both go through `writeState`, whose
       body is 157.
   - **The opt-outs are config keys**, `ecosystem:no_update_news` and
     `no_donation_nag` (`ConfigValues.cpp` 715-716). The files record what
     ran, not what the user dismissed (**R**).

The autogenerated-config banner is removed the same way as the welcome flag:
"Warning: You're using an autogenerated config! Edit the config file to get
rid of this message." (`src/config/lua/ConfigManager.cpp` 813,
[[source: hyprland-lua-config-manager]]), and the wiki says "Remove
`hl.config({ autogenerated = true })` to remove the autogenerated warning
banner." (`content/configuring/core/_index.md` 17,
[[source: hyprland-wiki-core-index]]).

**What this means for the request's analogy.**

- **What the logo and splash are.** They are permanent background art, not
  a launch animation that vanishes.
- **How they are turned off.** Each by a config key that defaults to
  showing it.
- **The nearest "do not show again" button for something on screen** is in
  a superseded app. It points at a config key and writes nothing.
- **Where the user's config is written,** it is the welcome app tidying a
  file Hyprland itself generated. The button that promises "never show it
  again" does not write it.

### 2.2 Other terminals

The sweep was read at source, at these pins:

- ghostty `246f702`;
- kitty `851cb80`;
- wezterm `3725482`;
- iTerm2 `964a75d`;
- Windows Terminal `6676c6f`;
- Warp `b2b7357`.

Every file cited below has a manifest key.

**The search.** One command per term per repository: `rg -i -n -uu -g
!.git -c --fixed-strings`. The sweep's probe runs it, through a wrapper
that prints each pin:
[[source: aj-obs-probe-tool]]
[[source: aj-obs-probe-run]]

There were 26 terms:

> splash, welcome, first launch, first_launch, firstLaunch, first run,
> first_run, firstRun, onboarding, don't show, do not show, don't ask, ask
> again, show again, remind, dismissed, dismiss, never show, tip of the
> day, tips, what's new, whats new, whatsnew, release notes, update
> window, logo

The positive controls, re-run for this revision with the same counts
([[source: aj-obs-probe-terminals]]):

- ghostty: `cursor-style`, 7;
- kitty: `cursor_trail`, 249;
- wezterm: `default_cursor_style`, 4;
- iTerm2: `Profiles`, 2362;
- Windows Terminal: `startupActions`, 292;
- Warp: `warpify`, 607.

**What the search found, and how far it was read.** The counts are
matching lines per repository, summed across files
([[source: aj-obs-probe-terminals]]).

| term | ghostty | kitty | wezterm | iTerm2 | Windows Terminal | Warp |
|---|---|---|---|---|---|---|
| splash | 0 | 3 | 1 | 7 | 9 | 3 |
| welcome | 5 | 2 | 7 | 81 | 9 | 508 |
| onboarding | 0 | 0 | 0 | 594 | 0 | 3330 |
| logo | 27 | 314 | 42 | 254 | 228 | 760 |

The `logo` hits were read by sampling. The sampling's script and output:
[[source: aj-obs-logo-sampling-tool]]
[[source: aj-obs-logo-sampling]]

- **ghostty, wezterm, iTerm2 and Windows Terminal.** Sampled by directory,
  the hits are substrings (`-NoLogo`, `kAEReallyLogOut`,
  `DLogOncePerLoggingSession`), Unicode and Nerd Font tables, package tile
  assets, one About-dialog icon (ghostty GTK, `logo-icon-name`), and
  iTerm2's `iTermLogoGenerator`, which "Generates a small iTerm2 logo with
  custom text, cursor, background, and tab colors" for Open Quickly's list
  (`sources/OpenQuickly`, iTerm2's top directory for the term). None is
  launch art.
- **kitty** has `window_logo_path` (below).
- **Warp.** Its 760 `logo` and 508 `welcome` hits were not each read. The
  findings below came from the onboarding and settings files that were.

**Every negative below is bounded by those terms and those reads.**

Warp's README says "Warp's client codebase is open source and lives in this
repository." (`README.md` 60, [[source: warp-readme]]). Its source is cited,
and its docs are labelled as docs. Whether the shipping binary matches
`b2b7357` is unknown.

| terminal | a logo at launch | a first-run screen | a dismissal, and where it is stored | the off switch |
|---|---|---|---|---|
| **Warp** | **yes**: the intro slide's logo on first run; the zero-state block's icon in each new session | onboarding slides, unskippable in source | completion and some banners in the private store; the zero-state block's "Don't show again" writes `settings.toml` in default builds | `terminal.show_terminal_zero_state_block`; `general.show_changelog_after_update` |
| wezterm | **yes, when an update is cached**: the update banner carries wezterm's app icon into each new pane | none found | no dismiss control in the banner code | `check_for_updates = false`; its default depends on the build |
| ghostty | none found | none found; first run writes a template config and shows nothing | the updater's permission prompt on the second launch, release builds only, stored in the app's user defaults | `auto-update` |
| kitty | none found; `window_logo_path` is a corner logo the user sets, `none` by default | none found | none; an update is an OS notification, once a version | `update_check_interval 0` |
| iTerm2 | none found | "What's New" once after an upgrade from 3.1-3.5, not on first run | tips, warnings and alerts, in its own settings store; tips and the move alert kept out of folder sync, warnings synced unless their identifier starts with `NoSync` | `NoSyncTipsDisabled`, "Disable Tips", the alert's checkbox |
| Windows Terminal | none found; no `uap:SplashScreen` in `Package.appxmanifest` | none found; first run writes `settings.json` and shows nothing | an info bar's "Don't show again", in `state.json`, beside `settings.json` | none for the info bar |

Details, each **R** at the pin:

- **Warp: a logo on first run, and an icon in each new session.**
  - **The intro slide.** It is the first onboarding slide. It builds a
    centred column whose first child is `Icon::WarpLogoLight` at 64 x 64.
    Then comes `ShimmeringTextElement::new("Welcome to Warp", ...)`, an
    animated shimmer at size 32, then a subtitle, then "Get started"
    (`crates/onboarding/src/slides/intro_slide.rs` 156-206; the strings at
    165 and 190; [[source: warp-intro-slide]]).
  - **When it shows.** Onboarding shows on first run. The source pins it
    unskippable: `false, // Always use unskippable onboarding.`
    (`app/src/root_view.rs` 2204, [[source: warp-root-view]]).
  - **Completion.** It is written to `HasCompletedOnboarding` in the
    private store (`root_view.rs` 1808, 1820-1824). That store's doc says:
    "Private settings are always stored in the platform-native store (e.g.
    UserDefaults on macOS, registry on Windows, JSON file on Linux) and
    never appear in the user-visible settings file."
    (`crates/warp_core/src/user_preferences.rs` 16-18,
    [[source: warp-user-preferences]]).
  - **The zero-state block.** It draws `Icon::Warp`, at the title's font
    size, beside a bold "New terminal session"
    (`app/src/terminal/view/zero_state_block.rs` 147-170,
    [[source: warp-zero-state-block]]).
  - **When the block shows.** It is inserted in each new session that
    meets all of these (`app/src/terminal/view.rs` 14512-14536,
    [[source: warp-terminal-view]]):
    - not restored;
    - no onboarding showing, and no onboarding callout view (14516);
    - no launch modal open;
    - not a subshell or ssh;
    - no plugin-instructions block (14519).

    It also needs the `AgentView` feature. That is in the default features
    (`"agent_view"`, `app/Cargo.toml` 625, [[source: warp-app-cargo]];
    mapped at `app/src/features.rs` 342-343, [[source: warp-features]]).
  - **Its setting.** `show_terminal_zero_state_block`, `default: true`,
    "Whether to show the AI zero-state block in new terminal sessions."
    It is shown only when AI is enabled (`should_show_zero_state_block`,
    `app/src/terminal/settings.rs` 176-184 and 222-224,
    [[source: warp-terminal-settings]]).
  - **Its "Don't show again" writes the user's config in default builds.**
    The button (`zero_state_block.rs` 296) sets the setting to false
    (339-346). The setting is public, with `toml_path:
    "terminal.show_terminal_zero_state_block"` (`terminal/settings.rs`
    176-184). Public settings go to the user's `settings.toml` when the
    default `settings_file` feature is on (`app/Cargo.toml` 679), and to
    the platform store otherwise.
  - **Other dismissals go to the private store,** for example the OSC 52
    and SSH ControlMaster banners' "don't show again"
    (`terminal/view.rs` 694-698, written at 23410-23411 and 23553).
  - **After an update.** The changelog is governed by
    `show_changelog_after_update`, `default: true`, "Whether the changelog
    is shown after an update." (`app/src/settings/changelog.rs` 5-13,
    [[source: warp-changelog-setting]]).
  - **The docs disagree with the source.** They say: "When you launch
    Warp, you’ll see a terminal session ready for input. You can optionally
    sign up for an account (top right), or skip and start using Warp
    immediately." (`llms-full.txt` 50100, [[source: warpdocs-llms-full]]).
    This is not reconciled with the unskippable slides in source, and both
    are reported.
  - **The separate Warp Agent CLI** has its own start screen, "showing a
    rotating ASCII object" (`llms-full.txt` 4173). Its switch is
    `appearance.zero_state.show_animation`, default true, "Whether the Warp
    Agent CLI zero state shows the rotating object and its starfield."
    (`app/src/settings/tui_zero_state.rs` 211-217,
    [[source: warp-tui-zero-state]]).
- **wezterm.**
  - **The banner.** At startup, if a cached update check is newer
    (`update.rs` 71-89), wezterm sets a banner: "WezTerm Update Available
    ... Click to see what's new". The banner carries wezterm's app icon,
    `ICON_DATA`, an `include_bytes!` of `assets/icon/terminal.png`
    (`wezterm-gui/src/termwindow/mod.rs` 97,
    [[source: wezterm-termwindow-mod]]). It is drawn inline as an iTerm
    image two cells high (`ITermDimension::Cells(2)`, `inline: true`;
    `wezterm-gui/src/update.rs` 93-131, the icon at 97-107 and 123, the
    text at 122; [[source: wezterm-update]]).
  - **Where it goes.** The mux writes it into each new pane's stream ahead
    of the pty's output (`mux/src/lib.rs` 320-321,
    [[source: wezterm-mux-lib]]).
  - **No dismiss control.** Neither the banner's code nor the mux's has a
    dismiss control (`update.rs`, `mux/src/lib.rs`).
  - **The off switch.** "Set `check_for_updates` to `false` to disable this
    completely" (`check_for_updates.md` 16-17,
    [[source: wezterm-check-for-updates]]). Its default is
    `cfg!(not(feature = "distro-defaults"))` (`config/src/config.rs`
    1626-1628, [[source: wezterm-config-3725482]]), so it is on in wezterm's own
    builds and off in builds with the `distro-defaults` feature, which
    wezterm recommends to distributions (`README-DISTRO-MAINTAINER.md`
    51-56, [[source: wezterm-readme-distro]]).
  - **`show_update_window`** "no longer has any effect"
    (`show_update_window.md` 20, [[source: wezterm-show-update-window]]).
- **ghostty.**
  - **First run.** It writes a config file and shows nothing: "If both
    files are not found, then we create a template file."
    (`src/config/Config.zig` 4294, [[source: ghostty-config-246f702]]). The
    template says so (`config-template` 3-4,
    [[source: ghostty-config-template]]).
  - **The updater prompt, in release builds only.**
    - **The source plist** sets `SUEnableAutomaticChecks` to `false`
      (`macos/Ghostty-Info.plist` 107-108, [[source: ghostty-info-plist]]).
      Sparkle skips the prompt when that key has a value: `if ([_host
      boolNumberForKey:...] != nil) { shouldPrompt = NO; }` (`SPUUpdater.m`
      407-409). It reads user defaults first and then the plist (`SUHost.m`
      400-404, [[source: sparkle-suhost-2-9-6]]).
    - **The release workflow** deletes the key
      (`ghostty/.github/workflows/release-tag.yml` 203,
      [[source: ghostty-release-tag]]).
    - **In a release build**, Sparkle 2.9.6 then waits for the second
      launch: "We wait until the second launch of the updater for this host
      bundle". It records `SUHasLaunchedBefore` in user defaults
      (`SPUUpdater.m` 415-420, [[source: sparkle-spuupdater-2-9-6]]).
  - **The config switch** is `auto-update`, whose doc says "If unset, we
    defer to Sparkle's default behavior, which respects the preference
    stored in the standard user defaults (`defaults(1)`)." (`Config.zig`
    3983-3984).
  - **The config-errors window's "Ignore"** persists nothing:
    `Button("Ignore") { model.errors = [] }`
    (`ConfigurationErrorsView.swift` 48,
    [[source: ghostty-configuration-errors-view]]).
  - **The animated logo is a command the user runs:** "The `boo` command is
    used to display the animation from the Ghostty website in the terminal"
    (`src/cli/boo.zig` 175, [[source: ghostty-boo]]).
- **kitty.**
  - **Updates.** "If an update is found, a system notification is displayed
    informing you of the available update. The default is to check every 24
    hours, set to zero to disable. Update checking is only done by the
    official binary builds." (`kitty/options/definition.py` 3096-3106,
    [[source: kitty-options-definition-851cb80]]). It notifies once per
    version, recorded in `cache_dir()/new-version-notifications-1.txt`
    (`update_check.py` 35, 92-94, [[source: kitty-update-check]]).
  - **A corner logo.** `window_logo_path` is "Path to a logo image ... The
    logo is displayed in a corner of every kitty window", default `none`
    (`definition.py` 1932-1942). It is a picture the user sets, not launch
    art.
- **iTerm2: its settings store is `NSUserDefaults`.**
  - **Reads.** `[[iTermUserDefaults userDefaults] objectForKey:key]`
    (`iTermPreferences.m` 1036-1037, [[source: iterm2-preferences]]).
  - **Where the file is.** Its remote-preferences alert names
    `~/Library/Preferences/com.googlecode.iterm2.plist`
    (`iTermRemotePreferences.m` 384).
  - **What `NoSync` does.** Keys prefixed `NoSync` are kept out of the
    synced preferences *folder*, `![key hasPrefix:@"NoSync"]` (169,
    [[source: iterm2-remote-preferences]]).
  - **Which dismissals sync.** The tips' keys carry the prefix, and the
    move alert is on the exempt list. A warning's suppression is keyed by
    its bare identifier (`iTermWarning.m` 1066-1072), so it syncs unless
    the identifier starts with `NoSync`. Either way the dismissals live in
    iTerm2's own settings store, not in a separate store.
  - **Tip of the Day.** It asks permission first, stored in
    `NoSyncPermissionToShowTip`. "Disable Tips" writes
    `NoSyncTipsDisabled`, the same name as an advanced setting
    (`iTermTipController.m` 21-23, 129, 259,
    [[source: iterm2-tip-controller]]; the button's title,
    `iTermTipWindowController.m` 32,
    [[source: iterm2-tip-window-controller]]).
  - **The move-to-Applications alert** shows AppKit's suppression checkbox
    and writes `moveToApplicationsFolderAlertSuppress`
    (`PFMoveApplication.m` 65, 202, 281,
    [[source: iterm2-pfmoveapplication]]).
  - **General warnings** offer "Suppress this message for ten minutes",
    "... for 30 days", "... permanently" and "Remember my choice"
    (`iTermWarning.m` 683-701, [[source: iterm2-warning]]).
  - **"What's New".** It is shown after an upgrade, when the stored previous
    version starts with 3.1-3.5 (`iTermOnboardingWindowController.m` 178,
    [[source: iterm2-onboarding-window-controller]]; the launch path,
    `iTermLaunchExperienceController.m` 111-113,
    [[source: iterm2-launch-experience]]).
- **Windows Terminal.**
  - **"Don't show again".** It is on an info bar shown after a connection
    fails, not at launch (`TerminalPage.cpp` 5002,
    [[source: wt-terminalpage]]; the label, `Resources.resw` 835,
    [[source: wt-resources-resw]]). It appends to `dismissedMessages`,
    "stored in `state.json`" (`TerminalPage.cpp` 5060-5071;
    `ApplicationState.h` 23 and 43, [[source: wt-applicationstate-h]]).
  - **Why `state.json`.** The project's spec gives the reason
    ([[source: wt-spec-8324]], 25-30):

    > "It is the author's opinion that the above-mentioned settings are
    > inappropriate for storage in the user's settings.json; they do not
    > need to be propagated immediately to other instances of Windows
    > Terminal, they are not intended to be edited, and storing them
    > outside of settings.json defers the risk inherent in patching the
    > user's settings file. Therefore, this document proposes that a
    > separate application state archive (`state.json`) be stored next to
    > `settings.json`."

  - **The contrast.** The close dialog's "Don't ask me again"
    (`Resources.resw` 535) writes the user's settings:
    `_settings.GlobalSettings().ConfirmOnClose(ConfirmOnClose::Never);
    _settings.WriteSettingsToDisk();` (`TerminalPage.cpp` 980-983). One
    program does both.
  - **First run.** It is detected at
    `CascadiaSettingsSerialization.cpp` 1231 (`firstTimeSetup =
    settingsString.empty()`). It resets `state.json` at 1261, marks
    `settings.json` for writing at 1267, and writes it at 1308-1310
    ([[source: wt-cascadia-settings-serialization]]). Nothing is shown.
  - **No splash.** The shipping `Package.appxmanifest` has no
    `uap:SplashScreen` ([[source: wt-package-appxmanifest]]). The asset
    generator still builds a 620 x 300 `SplashScreen` image
    (`Generate-TerminalAssets.ps1` 39, [[source: wt-generate-assets]]) that
    the shipping manifest does not reference. Only test and
    integrity-tool manifests name a splash screen.

**What the sweep says for baud.**

- **Warp is the precedent closest to the request.** It puts a logo
  on screen at launch: centred on the first-run slide, and as an icon in
  every new session's zero-state block. The zero-state block's "Don't show
  again" writes the user's config.
- **No other terminal swept draws a logo at launch except wezterm**, whose
  update banner carries its app icon into each new pane when a newer
  release is cached. This holds within the 26 terms and the files read.
- **On "do not show again", practice splits:**
  - **A separate state store.** Windows Terminal's `state.json`, on a
    stated rationale; Warp's private store for its banners.
  - **The app's own settings store.** iTerm2, its tips and move alert kept
    out of folder sync.
  - **The user's config written.** Windows Terminal's close dialog and
    Warp's zero-state block.
  - **A config key and no control.** kitty, wezterm, ghostty's
    `auto-update`, and Hyprland's logo and splash.

These are practice, not standard. No text read governs where a dismissal
lives.

### 2.3 sfte

`sfte.h` [[source: sfte]], searched by [[source: aj-obs-logo-tool]] ([[source: aj-obs-logo]]):

- **`splash`**: no match, case-insensitive. As a positive control, the same
  search finds `SFTE_TERM_ANIMATE_SCREEN` 30 times.
- **`logo`**: no logo-related match in any form.
  - Case-sensitive, it matches only "analogous" (954 and 15335).
  - Case-insensitive, it adds `XKB_MOD_NAME_LOGO`, the Super key (37297
    and 38786).
  - As a whole word, it matches nothing.

## 3. baud today

### 3.1 The logo assets

`assets/logo/README.md` describes the lockup as "`baud ❯ █` — mono
wordmark, amber prompt glyph, amber block cursor." The colour lockups
measure 300 x 130:

| element | `baud-lockup-dark.svg` | `baud-lockup-light.svg` | geometry (both) |
|---|---|---|---|
| ground | `#15171B` rect, baked in | none (transparent) | 300 x 130 |
| wordmark `baud` | `#F3F4F6` | `#1E2128` | x 40, baseline 88, 62 px, weight 500, letter-spacing -2.8 |
| prompt `❯` (U+276F) | `#F4A32C` | `#C8801F` | x 192, baseline 85, 54 px, weight 700 |
| block | `#F4A32C` | `#C8801F` | rect x 238, y 45, 22 x 46 |

What the README says, quoted with its em dashes:

- **Colour.** It gives each colour in oklch as well, for example amber in
  dark mode `oklch(0.78 0.16 68)`, and says "Light mode uses the deeper
  amber deliberately — `#F4A32C` fails contrast on white."
- **Type.** "JetBrains Mono — 500 for the wordmark, 700 for the prompt
  glyph, letter-spacing -0.045em." The SVGs "reference the font rather than
  embedding outlines".
- **Clear space.** "Minimum clear space on all sides = the cursor width
  (22px at the 300px lockup)."
- **Ground.** "Don't place the lockup on a mid-tone ground; the amber needs
  the dark or light ground to hold contrast."
- **Which is "the one with the color".** The mono variants (`-mono-black`,
  `-mono-white`) are the "single colour" ones, so the colour lockups are
  the one the request means.

Facts found reading them:

- **The "blinking" files do not blink.** The README says "Blinking runs at
  1.06s, the classic VT cursor rate". But `baud-lockup-dark-blinking.svg`
  and its siblings carry no `<animate>`, `<style>` or keyframes, and the
  block's `<rect>` holds only whitespace. A search for
  `animate|@keyframes|<style` over all twenty SVGs finds none, and the
  control `rect` finds all twenty (**C**, [[source: aj-obs-logo]]).
- **Geometry, by advance boxes (C).** Every figure in this bullet and the
  next comes from the font probe and its output:
  [[source: aj-obs-font-tool]]
  [[source: aj-obs-font]]

  The embedded JetBrains Mono has 1000 units to the em and a 600-unit
  advance for `a`, `❯` and `█`, with a cap height of 730.
  - **Wordmark.** At 62 px with -2.8 tracking its pitch is 34.4 px, and
    "baud" spans about x 40-180.
  - **Prompt.** At 54 px the `❯` box spans x 192-224.
  - **Block.** It is 22 x 46, or 0.35 em wide and 0.74 em tall at the
    wordmark's size, close to the cap height. That is not a full `█`,
    which fills the whole cell.
  - **Ink.** The ink gap from `d` to `❯` is about 25 px, computed from the
    glyphs' bounding boxes in the 400 and 700 faces. The ink gap from `❯`
    to the block is about 22 px. The lockup leaves 40 px each side.
- **The glyphs are in baud's embedded faces, and one weight is not.**
  - **The glyphs.** `baud-app/fonts/built/Regular.ttf` and `Bold.ttf` both
    map U+276F (glyph 846) and U+2588 (glyph 1185). The control letters
    `b a u d` map in both (**C**, by reading each `cmap`).
  - **The weights.** All six embedded faces are static. JetBrains Mono's
    four are at 400 and 700, and the Nerd Font and STIX faces at 400. None
    has an `fvar` table, so the wordmark's 500 is in no embedded face
    (**C**).
  - **Embedding Medium.** JetBrains Mono's static faces in the repository
    are 273,900-279,832 bytes (`baud-app/fonts/jetbrains-mono/`, version
    2.304). An added Medium face's size is unmeasured.
- **Where the logo is used now.**
  - **The Dock icon.** `scripts/bundle-macos.sh` renders
    `baud-icon-dock.svg` to an `.icns` with `rsvg-convert` (34-44). A bare
    binary, with no bundle, sets the SVG as an `NSImage` instead
    (`baud-app/src/app.rs` 4770, 4874-4884).
  - **The titlebar mark.** `baud-mark-amber-tight.svg`, rendered by AppKit
    as an `NSImage` (`icon_titlebar`, `app.rs` 4779).
  - **The lockup.** Nothing draws it.

### 3.2 The plasma is a cursor trail

`[cursor] trail = "plasma"` is built in `baud-render/src/plasma.rs`, with
the fragment `plasma_fragment` in `baud-present-macos/src/shaders.metal`.
It is recorded in its measurement. What each part is (**R**):

- **The body.** A tube fills kitty's collapsing quad. A sheath reaches
  `REACH` = 1 cell beyond it, and three filaments braid along the move's
  axis into the cursor's cell, the "hole". Nothing is drawn ahead of the
  cursor or inside its hole. Ions peel off the path in closed-form
  cyclotron spirals, 8-64 of them.
- **How it ends.** Everything dims as `(1 - k)^2` over `LIFE` = 0.55 s, and
  the last ion is out by 0.65 s at most. An ion is born within `SWEEP` =
  0.10 s and lives at most 0.55 s (**C**). The test
  `the_plasma_is_out_when_its_last_ion_dies` holds `(LIFE..=0.65)` for one
  seed.
- **What drives it.** An `Ignition`: the path from the cell the cursor left
  to the cell it reached, the age, the seed, the unit, and `rode`, how far
  the path has ridden a smooth scroll. The emitter is the quad, the hole is
  the cursor's rectangle, and the axis is the quad's move. Everything is a
  pure function of the ignition and its age (`effect.rs` 170-185, and the
  module doc).
- **Its colour.** "Every effect is the cursor's colour -- the one the quad
  trail is drawn in, a choice the project made -- and its intensity is its opacity"
  (`effect.rs`). The renderer's `cursor_fill` is the `OSC 12` colour if
  one is set, and otherwise the default foreground (`renderer.rs`,
  `refresh_screen`). By default the plasma is the foreground colour, not
  amber.
- **Where it draws.**
  - **GPU only.** "GPU only: the CPU path draws the emitter as the quad it
    is" (`effect.rs` 21-22).
  - **Linux** has no trail at all (commit `c2e20bed`, the quad trail). It
    drives no
    transition either: `linux.rs` has neither, and `config.rs` 349-350
    says "Only the macOS driver has a trail".
- **Its cost** (**M**, with commit `bc405703`, the plasma trail):
  - **The protocol.** `scripts/idle-cpu.py`, blink off, window unfocused,
    five rounds of ten seconds with the first dropped, an A/A copy of the
    plasma arm.
  - **The result.** Under a 30-column jump every 200 ms, the quad, the
    fire and the plasma read +5.9%, +5.2% and +5.8% of a core over no
    trail, "all inside the copy's 3.0% spread -- no result between them".
- **Its pipeline.** It is a late pipeline, "built the first frame the
  effect is drawn" (commit `f202aea3`, the fire as one effect of a
  family). A pipeline after the first is "about 7.5 ms with a cold shader
  cache and about 0.1 ms warm", inferred from per-pipeline figures and not
  measured (commit `c2e20bed`).
- **Still pending.** Choices recorded and not taken: the body fill, the
  flow direction, the ions' charges, the braid's width. A later priority
  list includes "plasma as the default trail on one implementation".
- **Its clips.** The trail's still and clip are tracked as
  `docs/cursor/plasma.png` and `docs/cursor/plasma.mp4`.

So "vanish with the plasma effect" does not name a built thing. The trail
is drawn along a move into a cursor, in the cursor's colour. A logo has no
move, and no cursor except its block.

### 3.3 The idiom for anything timed

The rule for anything timed (2026-09-25): never a sleep, a blocked
thread or a held frame. Anything timed is an effect whose end is scheduled
on the display link (macOS) or the poll timeout (Linux), and output keeps
drawing meanwhile.

The tick sets `pending = Dirty` while any of these is in flight
(`baud-app/src/app.rs`, the tick, about 4621-4686):

- an `SGR 5` blink flip;
- the cursor's blink (`caret.due`, 4632-4641);
- a flash's end;
- the ghost trail or the screen transition;
- the quad's `Chase::owes`;
- the scrollbar's reveal;
- the slide.

Each stops forcing frames when it settles, so "an idle screen returns to no
frames" (**R**). The durations baud already uses, which are not guidance:

- `TRAIL_S` = 0.1 s, the ghost trail;
- `TRANSITION_S` = 0.15 s, the alternate-screen fade;
- the plasma's 0.55 s life, out by 0.65 s;
- `SYNC_MAX` = 33 ms.

**The nearest built idiom to a fade is the screen transition**
(`baud-render/src/transition.rs`, `[screen] transition`, off by default,
"sfte's `SFTE_TERM_ANIMATE_SCREEN`"):

- what it is: "one translucent full-screen solid ... driven by a `progress`
  the driver computes from the display link's timestamp";
- how it fades: from alpha `0xE0` to nothing over 0.15 s;
- where it draws: "one translucent solid every path draws", so the macOS
  CPU path draws it too;
- what it holds: no frame (it is not a crossfade).

The Linux driver does not drive it (3.2). An opacity fade like it is what
WCAG excludes from "motion animation", and one of the HIG's Reduce Motion
practices.

### 3.4 Reduce Motion is already read, and an explicit choice wins

The rule of 2026-09-25, built in commit `bc7cd03`: Reduce Motion means no
trail, unless the config names one.

- **Two kinds of value.** `[cursor] trail` reads into `TrailPick::Default`
  or `TrailPick::Named(_)`. No `trail`, `trail = true`, and a value that
  names no trail are all `Default`, which "follow the system". Any trail by
  name, `off` included, wins: "A typo must not override an accessibility
  setting" (`baud-app/src/config.rs`, `TrailPick`).
- **When it is read.** The driver reads the preference at launch, "before
  the chase is built, so a launch under Reduce Motion never lights one"
  (`app.rs` 761-762). It polls it again on every tick beside the focus.
  `BAUD_MOTION=reduced|full` fixes it for checks.
- **What a change does.** Turning it on cuts a trail in flight, and costs
  one frame. The commit message words it "A switch cuts a trail in flight
  with one frame".
- **Its cost.** The launch's new stage, `reduce motion`, costs 38-44 µs in
  six launches (**M**, with commit `bc7cd03`).

A splash's Reduce Motion rule can take this same shape with no new
machinery.

### 3.5 The config is read, never written

The config reader (`baud-app/src/config.rs`, module doc):

- is "a small, dependency-free reader for a subset of TOML";
- looks for the file at `$BAUD_CONFIG`, then
  `$XDG_CONFIG_HOME/baud/config.toml`, then `~/.config/baud/config.toml`,
  then `./config.toml`;
- treats an unknown key as "ignored, and said, rather than fatal";
- treats sections as namespaces. The sections today are `[palette]`,
  `[cursor]`, `[screen]`, `[scroll]`, `[images]` and `[glass]`
  (`config.toml.example`). `theme` is a top-level key (31).

No code in `baud-app/src` writes a config or any persistent user state.

- **The only files it creates** are debug captures gated by environment
  variables: `BAUD_TRACE` (`app.rs`, `Trace::open`) and `BAUD_PTYLOG`
  (`pty.rs`, `reader`).
- **`NSUserDefaults`** is read for the system caret's rate (`os_cycle`)
  and never written.

(**R**, by grep for `fs::write`, `File::create`, `OpenOptions` and
`NSUserDefaults`.)

Note AG records that the development machine's
`~/.config/baud/config.toml` is gone (only `config.toml.old` ...). So a
"do not show again" that writes the config would, on that machine today,
*create* it.

### 3.6 Launch, as measured

None of these figures was taken for this note. Each column has its own
scope.

| stage, ms | commit `fb85b76`, cold boot | MEASUREMENTS, "Spawning the shell earlier (row 3, reverted)", base arm |
|---|---|---|
| pty spawned | 19 (superseded; see below) | 24.5 |
| window shown | 85 | 84.9 |
| first frame on the layer / first present | 92 | 87.9 |
| the shell's first bytes | 193 | 195.8 |
| the bench's wall, spawn to the first content, the loader included | 211 | 210.3 [201.8-214.6] |

**Scope of each column:**

- **The first column.** Commit `fb85b76`, 2026-09-13, measured on mains on
  one machine.
  - **The in-app stages** (19, 85, 92) were taken with the bench's default
    child, not the login shell.
  - **Only 193 and 211** are login-shell figures. So the window is up at
    92 and blank for a hundred milliseconds, and that hundred is the
    shell's, not baud's: `zsh -l -i -c exit` alone on that machine is
    **150-160 ms**, the devenv and direnv hooks in its rc files
    (2026-09-13).
- **MEASUREMENTS' column.** Apple M5 Max, macOS 27.0, on AC, two `.app`
  bundles through the login shell, seven rounds with the first discarded,
  an A/A copy, medians of six. Spawning the shell 15 ms earlier bought 8 on
  the prompt, under the floor, and was reverted (2026-09-18; the arm
  reverted by `1eb2e5d`).
  - **It corrects the 19.** The display-scale query alone is 23.6 ms, and
    "The shell was spawned at 24.5 ms because the grid needs the cell size
    in pixels, which needs the scale".
  - **The rc.** The shell's own rc took about 170 ms either way.

Both columns predate the Reduce Motion stage (`bc7cd03`, 2026-09-27). On a
shell without those hooks, the gap is unmeasured.

How the launch path runs (**R**; commit `fb85b76` and `app.rs`):

- **The first frame** is drawn synchronously after `show()`, waits for the
  GPU (`Done::Wait`), and is set on the layer in an explicit
  `CATransaction` (`app.rs` 5106 marks it).
- **After the first frame** come the menu (5179), the key responder (5181),
  the display link (5182), and the run loop (5185). The order is in 1.1.
- **The first frame's palette** comes from `window.is_dark()` under `theme =
  "auto"`. A `theme = "dark"` or `"light"` pins the half (section 4).
- **One window per process.** There is no "new window" in the app today
  (`Cmd+N` is a P4 item), so every launch is every window.

### 3.7 Keys reach the shell, and nothing on the way waits

A printable key goes from `Driver::key`, which returns `Handled::Interpret`,
to AppKit's `text()`, then `typed()`, then `App::send`. `App::send` calls
`crate::reads::input(&mut self.term, |t| t.key(e), |b|
self.outbox.push(b))` (`app.rs` 1024) and flushes the reply. A control
chord goes `Driver::key` to `typed()` directly.

The path *writes* animation state (`caret.input` restarts the blink), but
it *waits on* none (**R**). A splash that leaves the path alone meets the
HIG's "Let people cancel motion ... don't make people wait" for every key
that arrives once the run loop runs. A key typed before then is in 1.1.

## 4. Constraints no question decides

- **An overlay, never the grid.** The splash must not write cells into the
  terminal:
  - it would move the cursor and change what `CPR` reports;
  - it would be overwritten by, and would overwrite, the shell's output;
  - it would enter the scrollback.

  It is drawn the way the trail and the transition veil are: records over
  the cells, owned by the app and the renderer, invisible to `baud-core`.
  wezterm's update banner, written into each new pane's stream ahead of
  the pty output (2.2), is the practice baud declines, for those reasons.
- **It must delay nothing that is measured.** The pty spawn, the first
  present and the run loop's start are to happen as they do today. That is
  the requirement, and it is unmeasured until the acceptance test runs. The
  acceptance test is
  `scripts/startup-bench.py` with an A/A copy: first present and first
  content inside the A/A spread with the splash on (section 7).
- **Input goes to the shell at once** (3.7). Output draws under the splash
  while it runs. Neither is held.
- **It is scheduled.** The splash's end is a display-link timestamp, and it
  owes frames only while in flight, like the trail.
- **It follows the theme half the renderer resolved, not the window's
  appearance alone.** The top-level `theme` key is `auto`, `dark` or
  `light` (`ThemeChoice`, `config.rs`; `config.toml.example` 31). Only
  `auto` follows `window.is_dark()`, and a pinned half wins over the
  system.
  - The dark lockup's colours go on the dark half, and the light lockup's
    on the light half.
  - It is drawn without the dark lockup's baked `#15171B` ground, which
    would be a rectangle on the glass.

## 5. The options

Each option has its total visible time where it can be stated, its cost,
and its standing against the guidance in section 1. Where nothing governs,
that is said.

### 5.1 When the logo starts to vanish

V below is the vanish's own length (5.2).

| | rule | logo + vanish on this Mac | cost | guidance |
|---|---|---|---|---|
| **E1** | a fixed time T from the first present, output drawing beneath | T + V; T is a chosen parameter | one timestamp; frames during the vanish only | a view that auto-dismisses on a timer: against "Minimize use of time-boxed interface elements... Prefer dismissing views with an explicit action" (1.1). Against the decision's "fast so it feels snappy": depends on T |
| **E2** | the shell's first *visible* output starts the vanish | at least about 100 ms + V (3.6); on another shell, U | a new check (below), cost unmeasured (U) | not a timer, and not an explicit action. Against "at a glance": whether ~100 ms is enough is unmeasured (U). Against "snappy": as short as the shell; E4 is never longer (it ends on E2's trigger or its cap), and E3, or E1 with a small T, can be shorter |
| **E3** | the first key starts the vanish; the key goes to the shell | until the user types, + V | one flag on the key path, which writes state and waits on none | an explicit action: "Prefer dismissing views with an explicit action"; "Let people cancel motion". Alone it is unbounded, which departs from the decision's "fast so it feels snappy" and from "at a glance" |
| **E4** | E2 or a cap C, whichever is first, plus E3 | at most C + V | E2's and E3's, plus one timestamp | the cap is a timer (as E1), the rest event-driven (as E2 and E3). Against "snappy": bounded by C |

**What counts as "the shell's first output" for E2.** The first change to a
non-blank visible cell, not the first byte. A shell's first bytes are often
invisible:

- OSC 7's directory;
- a title;
- mode sets;
- a hook's escape sequences.

If any byte counted, the splash could end before anything is drawn.

**E2's check has no home yet (R).** Row damage is computed only in
`Renderer::convert` (`renderer.rs` 1567-1608), on the instance and CPU
paths. The default cell path copies the screen for the GPU and computes no
row damage (`app.rs`, around 1380-1530), and the first frame and resizes
are `Damage::Full`. On the cell path, E2 needs its own test, for example
the first non-blank cell that differs from the first frame's. Its cost is
unmeasured (U).

A summary of the request, written later and not part of it, reads "any key
or output ends it early". That is E2 plus E3. On this box
it gives the logo at least about 100 ms before it starts to vanish (**M**
for the gap, **U** for whether that reads as "at a glance").

### 5.2 How it vanishes ("which plasma")

| | what it is | V, the vanish's length | cost | guidance |
|---|---|---|---|---|
| **P1** | a new member of the effect family over the logo | the plasma's 0.55-0.65 s if its life is kept (U) | a new `Effect`: fragment function, Rust reference, oracle scenes, one more late pipeline (3.2), and a non-cursor colour (5.3) | motion animation: conforms if Reduce Motion takes it away (5.5); 2.3.1: inherits the trail's open body question (1.3, not settled); its knots and ions pass on area on the same model if the braid keeps the trail's filament widths; to be counted on rendered frames for the design (U) |
| **P2** | the logo's block flies to the shell's cursor on the plasma trail; the wordmark fades (an opacity fade, which departs from "vanish with the plasma effect" for the wordmark, as P3 does for the whole logo) | the trail's 0.55-0.65 s from ignition; the flight's own time is the chase's, not stated (U) | a synthetic chase start from a rectangle the cursor never occupied; the trail's pipeline and shader as built; the wordmark's fade (P3's path); under C1, a fill that is not `cursor_fill` | motion animation, as P1; its braid is the trail's, cursor-wide, so the trail's 2.3.1 model (1.3) carries over, and is not settled |
| **P3** | an opacity fade of the logo, the transition veil's idiom | 0.15 s on the transition's timing, or a chosen length | an overlay with an alpha; the veil exists on macOS, and Linux needs new timing on the poll timeout (3.3) | not motion animation (WCAG's definition), and one of the HIG's Reduce Motion practices; at most one flash. As the vanish itself it departs from the decision's "vanish with the plasma effect" |
| **P4** | the classic demoscene plasma, a field of summed sines through the logo's pixels | unstated (U) | a new shader, reference and oracle; nothing in baud draws it | motion animation, as P1; 2.3.1 not computed (U) |

Against the decision's "fast so it feels snappy": whether 0.55-0.65 s of
plasma, or 0.15 s of fade, reads as snappy is unmeasured and a choice
(U).

More on each:

- **P1.** An `Effect` whose region is the logo's box and whose hole is the
  logo's block. The tube and sheath fill the box and dim as `(1 - k)^2`,
  the filaments braid along the wordmark into the block, and the ions peel
  off. It reuses the family's records, uniform and pure-function
  discipline. What is new is an ignition with no move, so the braid's axis
  is the logo's baseline.
- **P2.** The logo's `█` is a cursor in the logo's own design, and the
  plasma trail exists to carry a cursor from one place to another. It
  reuses `Ignition`, `plasma.rs`, the shader and its oracle as built. What
  changes is how a chase starts (from a rectangle), and under C1 the fill.
  - **Reduce Motion.** Under Reduce Motion the default trail is already
    off (3.4), so P2 falls to whichever form 5.5 picks.
  - **Colour.** The block's colour is 5.3's question.
- **P4.** The tracked `docs/cursor/plasma.png` and `.mp4` are the trail's,
  and `tests/previews/README.md` uses ImageMagick's `plasma:fractal` only to
  make a synthetic test image. It is listed because "the plasma effect"
  could mean it (**U** which one the request means).

P1, P2 and P4 are motion animation, and P3 is not. P3 applies wherever a
fade is chosen: under Reduce Motion (Q5), or on a path with no GPU effects
(Q8).

### 5.3 Colour

No criterion read governs a decorative logo's colour. SC 1.4.3 exempts it
outright, under the heading "Logotypes": "Text that is part of a logo or
brand name has no contrast requirement." ([[source: wcag22]], SC 1.4.3).

- **The logo** takes the lockup's colours by the resolved half: `#F3F4F6`
  and `#F4A32C` on dark, `#1E2128` and `#C8801F` on light (3.1). This is the
  request's "the one with the color". Cost: two colour pairs picked by the
  half the renderer already resolves.
- **The plasma** has two options:
  - **C1, the logo's amber.** The effect family has no such rule today:
    every effect is "the cursor's colour", decided on 2026-09-25.
    Cost: a fill per effect instead of the shared `cursor_fill`, at the
    instance builder.
    - **2.3.1.** A red flash needs a saturated red in either state, so the
      fill's own R/(R+G+B) (0.54 and 0.56) is not the test. The test is the
      fill over a saturated-red ground, and the model computes it
      ([[source: aj-obs-flash-area]], **C**). Over any pure-red ground from
      `#080000` up, the amber's largest u'v' step is 0.184 (`#F4A32C`) and
      0.178 (`#C8801F`), under 0.2. Over near-black red grounds, `#010000`
      to `#030000`, it exceeds 0.2 at opacities around 0.05: 0.217 and
      0.214 over `#010000`. Those grounds are reds by the ratio but nearly
      black, and whether the working definition means to count them is not
      settled (U).
  - **C2, the cursor's colour.** This keeps the family's rule. By default
    that is the foreground, so the plasma would be the wordmark's grey or
    near-black, not amber. Cost: none.
    - **2.3.1.** It follows `OSC 12`, which can make it a saturated red,
      the case 1.3 leaves open. The default foreground over a red ground is
      a red-flash case too: over `SGR 41`'s `#CD0000` it crosses the 0.2
      step at opacity 0.645, and over `#1A0000` at 0.150 (1.3).
  - **Under P2,** C2 would draw the logo's block in the cursor's colour, the
    foreground by default. That departs from the decision's "the logo should
    be the one with the color".
- **The ground is not always dark or light.** A `[palette] light` or `dark`
  file can name any background, and the glass shows the desktop through
  it, so baud cannot promise the README's ground (3.1). There are three
  options:
  - **The logo as is.** No cost.
  - **The variant by the resolved background's luminance**, rather than by
    the half's name. Cost: one luminance test at launch.
  - **The mono variant on a mid-tone ground.** This departs from "the one
    with the color".

  All three conform, since WCAG exempts logotypes. The README's warning is
  brand guidance.

### 5.4 Drawn as cells or as pixels

Nothing read governs this choice.

- **L1. Cells.** `baud ❯ █` drawn as eight cells at the terminal's cell
  size, from the embedded faces, centred on the grid as an overlay.
  - **Cost.** Six glyph instances (the two spaces draw nothing) at the cell
    size the atlas already holds, in an overlay layer.
  - **Where it departs from the lockup's drawing, in five ways:**
    - the wordmark is at 400, not 500;
    - there is no -0.045 em tracking;
    - the spaces are full cells, where the lockup's ink gap is about
      25 px at 62 px;
    - the `█` fills the cell, where the lockup's block is 0.35 x 0.74 em;
    - it is eight cells wide whatever the window.
  - **Unstated.** The prompt's size and weight. The lockup draws `❯`
    smaller (54 against 62 px) on a lower baseline (85 against 88), at 700.
    In cells it would be the wordmark's size, at 400, or at 700 from
    `Bold.ttf`.
- **L2. Pixels.** The lockup's own geometry (3.1), scaled to a chosen
  fraction of the window.
  - **Cost.** The glyphs drawn by baud-font at a scaled size that is not
    the cell size, so new atlas entries at a second size. Whether the
    atlas's strikes take a second size as built is unverified (U). Then a
    solid block and an overlay layer.
  - **Where it departs:** the wordmark's weight, 400 unless a Medium face
    is embedded. A Medium face is OFL, as JetBrains Mono's other faces are,
    and its size cost is unmeasured (3.1).
  - **Size.** The HIG gives none, and the README gives only the clear
    space.

### 5.5 Reduce Motion

Follow `TrailPick` (3.4): an unset or default value follows the system, and
a named value wins.

| | under Reduce Motion | cost | guidance |
|---|---|---|---|
| **R1** | no splash | none | meets "reducing automatic ... animations" by removing it; under Reduce Motion it departs from the decision's "start with the ... logo" |
| **R2** | the logo, then an opacity fade (P3) | P3's | meets it with one of the five listed practices ("Replacing transitions ... with fades"); not motion animation by WCAG's definition |
| **R3** | the plasma anyway | none | departs from the HIG, unless the reader named it in the config, as a named trail survives today |

R1 and R2 meet the HIG's requirement equally. A later summary, "Reduce
Motion: show without the plasma, or not at all", names R2 then R1, but it
is not the request's words.

### 5.6 The key

Nothing read governs the name.

- **Candidates**, following the config's sections (3.5):
  - `[launch] splash`;
  - `[screen] splash` (beside `transition`).
- **Values that mirror `[cursor] trail`:**
  - a named vanish: `"plasma"` and `"fade"`;
  - `"off"`;
  - `true` or unset for the default, which follows Reduce Motion.

So `splash = "off"` turns it off whatever the system says.

- **The default.** On or off is open. Hyprland's logo and
  splash keys default to showing, and baud's `[screen] transition`
  defaults to off. Shipping it off departs from the decision's "i'd like baud
  to start with the ... logo ... and a config option to turn it off",
  which reads as on by default with a switch to turn it off.
- **The placeholder.** `[launch] splash` is a placeholder from a later
  summary.
- **Cost.** One key in the reader, a `TrailPick`-shaped enum, its tests (a
  typo follows the default), and a line in `config.toml.example`.

### 5.7 "Do not show again"

What writes, and where:

| | mechanism | writes | precedent | cost |
|---|---|---|---|---|
| **K** | the key only, documented; nothing in the app writes it | nothing | Hyprland's logo and splash keys and qtutils' button pointing at `no_update_news` (2.1); kitty's `update_check_interval`; wezterm's `check_for_updates` (2.2) | none beyond 5.6 |
| **S** | a separate state store the app owns: `NSUserDefaults` under baud's domain, or a state file under `$XDG_STATE_HOME/baud/` | app state, never the config | Windows Terminal's `state.json`, with its spec's reason; Warp's private store for its banners (2.2) | the app's first persistent write; a store per platform; a precedence rule against the key |
| **W** | the app edits the user's `config.toml` | the user's file | Hyprland's welcome "Finish" rewriting `hyprland.lua` (2.1); Windows Terminal's close dialog; Warp's zero-state block (2.2) | a TOML writer that keeps comments and order, which does not exist; on the development machine it creates a config that does not exist (3.5) |

Hyprland's `lastVersion` and `lastNag` are automatic state, not dismissals
(2.1, item 4), and are not precedent for S. iTerm2 keeps its dismissals in
its own settings store, which is its configuration (2.2), so it is not
precedent for S either.

**Costs, in more detail:**

- **W.** baud's config code is a subset reader with no writer. W stays
  open, since writing the user's config needs their OK.
- **S.** Under S, the precedence of the state against the key is a decision
  of its own. One rule is that the key wins when it is set, the
  `TrailPick` shape again.

**What the user would press, if anything writes:**

- **An in-splash control**: a button, or a hint "press ... to stop showing
  this".
  - **Time.** A sub-second splash leaves no time to read or aim.
  - **Keys.** If a key starts the vanish and goes to the shell (E3, 3.7),
    a key cannot also mean "never again" without being taken from the
    shell.
  - **Cost.** Hit-testing or a key binding on the overlay, a longer splash
    to make it usable, and S or W behind it.
  - **Guidance.** The time-boxed paragraph (1.1) names "people who need
    longer to process information". A control that has to be caught
    inside a view that dismisses itself is hardest for them. Nothing read
    forbids it.
- **A menu item**, for example "Show Logo at Launch", toggled. It lives
  outside the splash's second.
  - **Cost.** A change and a release of baud's window crate, because
    baud's menu is that crate's and is extended there, never duplicated.
    It also needs S or W behind it to remember the choice.
  - **Guidance.** Nothing read governs it. The HIG's tutorial sentence asks
    that a skipped item be "easy for people to find if they want to view
    it later" (1.1), which a menu item meets.
- **None**, the key only (K).

**Every launch, or the first only.** A first-launch-only splash is the
HIG's onboarding placement, and it needs S to remember. The project's "i'd
like baud to start with the ... logo" reads as every launch. A
first-launch-only splash is a reading of those words, marked as such.

**Guidance.** Nothing read governs where a dismissal lives. The HIG's
nearest text is the tutorial sentence (1.1).

### 5.8 Linux and the CPU path

The macOS CPU path (`BAUD_GPU=0`, or a Mac without Metal) draws no effect,
only the emitter's quad. Drawn that way, P1 would be a rectangle
collapsing. The Linux driver has neither a trail nor the transition.

| option | cost | standing |
|---|---|---|
| P3 (the fade) on every path without GPU effects | the veil on macOS's CPU path as built; on Linux, new timing on the poll timeout | conforms (1.1, 1.3) |
| no splash there | none | conforms; on those paths it departs from the decision's "start with the ... logo" |
| the vanish waits for the Linux GPU half | none now | conforms; meanwhile there is no splash on Linux, which departs from the decision's "start with the ... logo" there |

The rule "same pixels on every OS" is the project's, not a standard. It asks the paths to match, which favours the fade or no splash.

## 6. Costs, and what is unmeasured

**Recorded costs** (none taken for this note):

- **A late pipeline's build**: 7.5 ms cold and 0.1 ms warm, inferred and
  not measured (3.2).
- **A trail effect's frames** cost what the quad's do, inside a 3.0% A/A
  spread, under 3.2's protocol.
- **Reduce Motion's launch read**: 38-44 µs in six launches (3.4).
- **A glyph-map miss**: about 4.3 µs. MEASUREMENTS, "The glyph map's
  first-frame misses (row 8, measured, no change)", derives it as the first
  frame's convert less the walk, over its misses. Its scope is the M5 Max,
  the app's default CJK stack, 200x50 at 14 px.

**Unmeasured:**

- the splash's draw on the first frame;
- its effect on first present, the run loop's start and first content;
- the late pipeline's actual build at launch;
- the shipped trail's flashes on rendered frames, under retargets and for
  a saturated-red cursor colour (1.3);
- the plasma over a logo-sized region, and its per-pixel transition rate;
- the gap on any shell but the development machine's;
- whether about 100 ms of logo is "at a glance";
- P2's flight time and P4's length;
- whether a key typed before the run loop starts is queued or lost;
- the binary-size cost of an embedded Medium face.

**Where the costs land.** With the logo on frame one and the vanish on later
display-link frames, the late pipeline's build (P1, P2) lands on the first
vanish frame and not on the launch path. That placement is the design's to
keep, and it is not yet measured.

## 7. Red-first tests and the acceptance test, before any code

- **The acceptance test.** `startup-bench.py` with `BAUD_CMD=shell
  BAUD_BENCH_STAGE="first content"`, three arms: base, splash on, and an
  A/A copy. First present and first content must lie inside the A/A spread
  with the splash on (the arms built by
  `git archive`, never a worktree).
- **A key during the splash** reaches the pty on the same frame as with the
  splash off. The latency instrument's key-to-write must match the off arm.
- **Output during the splash** is drawn under it on the frame it arrives
  (no held frame). The grid, `CPR` and the scrollback are byte-identical
  with the splash on and off.
- **Under `BAUD_MOTION=reduced`** the default draws R1 or R2, whichever is
  chosen, and a named vanish draws itself. This mirrors
  `reduce_motion_turns_the_default_off_and_leaves_a_named_trail`.
- **`splash = "off"`** draws nothing and owes no frame. A typo follows the
  default, as `trail` does.
- **An instrument, `BAUD_SPLASH=off|on`**, in the idiom of `BAUD_MOTION` and
  `BAUD_SCROLLER`. It fixes the splash for checks without changing a
  config. Without it, a default-on splash would land in the pixel oracle's
  frames, the screenshot tooling and the startup bench's first frames.
- **The splash owes no frame after it ends.** The idle frame count matches
  the off arm, as the trail's check counts frames.
- **The light and dark first frames** draw the light and dark lockup's
  colours, in a pixel check against the resolved half. Under `theme =
  "auto"` the check runs under both appearances. Under a pinned `theme =
  "dark"` it runs on a light system too, which is the case
  `window.is_dark()` alone would get wrong.
- **E2's trigger** is the first visible damage. A shell whose first bytes
  are OSC 7 and a title does not start the vanish until its prompt is
  drawn.
- **The shipped trail's flashes, counted on rendered frames, before any
  plasma vanish (P1, P2) is built on it.** Count per 341 x 256-point
  window at 1x, from the GPU path's frames:
  - a long move without a retarget;
  - a back-and-forth retarget within `REIGNITE`;
  - each at the default and the largest supported fonts;
  - each with the cursor colour white, black and a saturated red
    (`OSC 12`), over a light and a dark ground;
  - a saturated-red ground (an `SGR 41` region, a dark-red palette
    background) under the default cursor colour.

  It must show no more than three general or red flashes a second, or
  flashing within 25% of the window (21,824 at Note 1's estimate). Then
  the 1.3 model can be compared with what the frames show.
- **If P1, P2 or P4:** the same count for the splash's own region, at the
  largest size the window allows.
- **If P1:** the effect joins `Effect::ALL`, and the family's generic gates
  (`every_effect_has_its_fragment_in_the_library`, the oracle's scenes)
  hold it the day it is added.

## 8. Open questions

The request's own words are only those quoted at the top. Later summaries
of it are not, and no question below rests on them.

1. **When does the logo start to vanish?** The options:
   - **E2+E3**: on the shell's first visible output or the first key. The
     output gives about 100 ms of logo here, or less if a key comes first
     once the run loop runs.
   - **E4**: the same, with a cap.
   - **E1**: after a fixed time, with the prompt drawn beneath.

   How each stands:
   - **E1 and E4's cap** are timers. The HIG's time-boxed guidance prefers
     an explicit action to a timer.
   - **E2+E3** is not a timer, and the output trigger is not an explicit
     action. Whether about 100 ms of logo meets "at a glance" is
     unmeasured (U).
   - **E3 alone** is unbounded, which departs from "fast so it feels
     snappy".

   Each option's total, logo time plus vanish, is in 5.1. No text gives a
   duration.
2. **Which plasma?** The options:
   - **P1**: a new effect over the logo's box, braiding into the block.
   - **P2**: the block flies to the shell's cursor on the plasma trail;
     the wordmark fades, which departs from "vanish with the plasma
     effect" for the wordmark.
   - **P4**: the classic sine plasma over the logo's pixels.

   P3, the fade, applies if a fade is chosen for Reduce Motion (Q5) or for
   a path without GPU effects (Q8). As the vanish itself it departs from
   "vanish with the plasma effect". Whether 0.55-0.65 s of plasma reads as
   snappy is open (5.2). Every plasma option waits on the
   shipped trail's 2.3.1 count on rendered frames (1.3, 7).
3. **The colours.**
   - **The plasma.** The logo's amber (C1, which the effect family has no
     rule for today), or the cursor's colour (C2, the 2026-09-25 rule: the
     foreground by default; under P2 it departs from "the one with the
     color").
   - **The logo on a mid-tone ground.** A custom palette or the glass can
     give one. The options are the logo as is, the variant by the
     background's luminance, or the mono variant, which departs from "the
     one with the color". WCAG exempts logotypes, so all three conform.
4. **Cells or pixels?** The options:
   - **L1**: the 8-cell `baud ❯ █`.
   - **L2**: the lockup's geometry scaled.

   Where each departs from the lockup's drawing:
   - **L1, in five ways.** Weight 400, no tracking, full-cell spaces, a
     full-cell block, and a fixed eight-cell width. The prompt's size and
     weight are also open (5.4).
   - **L2, in one.** Weight 400, unless JetBrains Mono Medium is embedded.

   L1 costs six glyph instances at the cell size. L2 costs a second glyph
   size in the atlas, a solid and an overlay. For L2: at what size, and is
   Medium embedded?
5. **Under Reduce Motion:** no splash (R1), or the logo with a fade (R2)?
   Both meet the HIG's requirement. R1 departs, under Reduce Motion, from
   "start with the ... logo". A named value in the config wins either way,
   as for the trail.
6. **The key and its default.** Its name (`[launch] splash` or `[screen]
   splash`), its values (`"plasma"`, `"fade"`, `"off"`), and whether it
   ships on or off. Off departs from "start with the ... logo ... and a
   config option to turn it off".
   - **Decided: on by default**.
     No text governs; it matches the request's words, read as on by default
     with a key to turn it off. Leaving the key unset shows the splash,
     except under Reduce Motion, which question 5 decides; a value that
     turns it off wins whatever the system says (5.6).
   - **Still open:** the name (`[screen] splash` was the nearest, and not
     settled) and the values; the named vanishes wait on how the logo
     vanishes (question 2) and on Reduce Motion's look (question 5).
   - **Unbuilt, and why.** No splash exists (questions 1 to 5 are open), and
     the key cannot be written without its name. The `BAUD_SPLASH=off|on`
     instrument of section 7 and the startup bench's arm would switch off
     something that is not there, so they land with the splash, with
     section 7's tests red first. The splash's own flash count (note AL)
     waits on it too.
7. **What "do not show this again kinda thing" asks for.** The request's
   words can be read two ways.
   - **The config key itself (K).** This is how Hyprland's logo and splash
     are switched off. Both Hyprland controls worded as a dismissal write
     nothing:
     - qtutils' button "Don't show this when I update" (`main.qml` 94)
       opens a dialog that names the config key (`main.cpp` 54);
     - the welcome app's "never show it again" button (guiutils welcome
       `main.cpp` 55) only closes the window (774-781).
   - **A dismissal control that records the choice.** Warp's zero-state
     "Don't show again" writes `settings.toml`. Windows Terminal's info bar
     writes `state.json`.

   If a control is wanted:
   - **Where it records the choice.** A state store outside the config
     (S), or the user's config (W).
   - **What the user presses.** A menu item, which costs a change and a
     release of baud's window crate, or a key in the splash, which
     conflicts with keys going to the shell.
   - **How often it shows.** Every launch, which is how "start with the
     ... logo" reads, or the first only, a reading that needs S.
8. **Linux and the CPU path.** The fade there, no splash there (which
   departs from "start with the ... logo" on those paths), or wait for the
   Linux GPU half?

**This draft's leans, before renders** (inputs to the decision, not
recommendations):

- **1: E4.** It is bounded both ways, and it holds the logo for the shell's
  own startup time. Its cap is a timer, against the time-boxed guidance,
  and the cap's value is a choice.
- **2: none.** It is a look, and a look is decided from renders. P2 is the
  only option whose shader and oracle exist.
- **3: none.** It is the 2026-09-25 rule that every effect is the cursor's
  colour against the request's own "the one with the color".
- **4: L2 at weight 400.** It is closer to the lockup and adds no face.
- **5: none.** R1 and R2 meet the requirement equally, and only the
  request's intent can break the tie. R2 keeps the logo, and R1 departs
  from "start with the ... logo".
- **6: no lean** on the name.
- **7: none between the readings.** If a control is wanted, the precedents
  divide by what they write. Some write nothing and point at a key
  (qtutils). Some write app state outside the config (Windows Terminal's
  `state.json`, Warp's private store). Some write the user's file (Warp's
  zero-state block, Windows Terminal's close dialog, Hyprland's welcome
  "Finish").
- **8: the fade,** which matches across paths.

Before any of these goes for decision, it needs:

- renders of the options;
- the texts above quoted beside them;
- a recommendation per question.

The status line records what was decided.

## 9. The emerge (2026-10-28)

**What the user sees.** The logo is on screen from the first frame,
faint inside a burst of the amber plasma, and is drawn whole from its
left edge to its right as the plasma's tail sweeps across it, the block
last, whole about 0.25 s after the first frame; once the emerge's plasma ends at 0.63 s it holds whole for 0.4 s, then goes back into the plasma from its
left edge to its right, the block last again; all of it gone about 1.7 s
after the first frame. Keys go to the shell and its output draws under the logo
throughout. Under Reduce Motion it is as before: whole, then the fade.

The requests, verbatim: "i think when baud start, we should show the logo
immediatly, it should popup with the plasma effect, then disapear with
it... right now it starts displayed and immediatly vanishes with plasma,
its good but it needs more", and "it should apear from left to right and
disapear from left to right".

**How it is built.** The emerge is the vanish's own plasma, P1 at kitty's
constants, with the ink the other way: each glyph stands at 0.25 of its
opacity (`GHOST`) and is drawn whole as the quad's trailing edge crosses
it. No new shader; the pixel oracle holds every arm to it at four ages
(`the_splash_draws_alike_on_every_arm`). The vanish was not strictly left
to right: its block began to starve on the first frame while the prompt
was still whole. The block now stays whole until the trailing edge is
within its own width, then starves with the tube as before
(`both_sweeps_run_left_to_right_the_block_last`).

**The hold is the flash bound.** Counted on rendered frames over the
whole life ([[source: aj-obs-emerge-census]]), the two plasmas land in
one second unless the hold keeps them apart: at kitty's pace a 0.3 s hold
flashes some pixel four times in a second on three of the four grounds,
and faster emerges flash up to six. The shortest passing holds tried:
0.4 s at kitty's pace, 0.6 s at 1.4 times it, 0.7 s at 2.1 times it. At
0.4 s no pixel flashes more than three times at 64x20 cells at 1x at
60 Hz, or at 200x50 at 2x at 60 and 120 Hz -- three, so with no margin.

**The choices**, made from frame strips of each option:

- **Form: the trailing edge** ([[source: aj-obs-emerge-trail-kitty]]),
  over a plasma cursor flying from the left edge to the block with the
  logo drawn at its head ([[source: aj-obs-emerge-lead-kitty]]). The head
  runs on kitty's fast constant, so that logo is whole by the third frame
  at 60 Hz -- close to the "starts displayed" the request asks more of --
  where the trailing edge sweeps the glyphs over 9 frames, at the speed
  of the vanish's own sweep.
- **Pace: kitty's**, holding 0.4 s, over 1.4 and 2.1 times it holding
  0.6 and 0.7 s ([[source: aj-obs-emerge-trail-fast14]],
  [[source: aj-obs-emerge-trail-fast21]]): every option starts the vanish
  at 1.00-1.05 s, and only kitty's keeps the pace question 2 of the
  status records.
- **Reduce Motion: the logo whole**, R2 unchanged, over a 0.15 s fade-in:
  the request asks for the logo at once, and a fade-in is an animation R2
  does not have.
- **Trigger: the clock alone, the lean.** The first visible cell or the cap always
  falls before 0.5 s and the hold ends at 1.03 s, so keeping them would
  change no frame; under Reduce Motion, which has no emerge, they stay.

**Unmeasured.** This replaces section 6's placement: under full motion the emerge draws the plasma on the first frame, so its pipeline's build is on the launch path; under Reduce Motion it is still built while the logo is held. The plasma's pipeline now builds on the first frame, not
on the vanish's first: section 6's late build (7.5 ms cold, 0.1 ms warm,
inferred) lands on the first present. No launch was timed for this change.

## 10. When it starts, and how long it stands (2026-10-29)

**What the user sees.** Nothing of the logo until the window is on
screen; then it comes out of the plasma left to right as before, is whole
about 0.25 s in, and starts to go at 0.48 s, left to right, in a faint
amber glow with no sparks; gone about 1.1 s after it began. It stood whole
about 0.78 s before (0.25 s to 1.03 s), and about 0.23 s now. Keys and
output are as before; under Reduce Motion the logo is whole from the
first frame, and its 0.5 s cap counts from the window's being shown.

The request, verbatim (2026-10-28): "ok the baud logo now starts too fast
and waits too much, so i think it should start after the window is fully
displayed, and not wait so much with full display".

**When the window is shown.** On macOS, the first display-link tick at
which a frame has reached the layer, the plasma's pipeline is built, and
the window is `isVisible` ("visible onscreen") with its occlusion state
`.visible` ("at least part of the window is visible")
([[source: apple-nswindow-isvisible]],
[[source: apple-nswindow-occlusionstate]]). A frame reaches the layer in
three ways, and each marks it: the first frame, drawn at once on either GPU
path; every frame on the CPU path; and a GPU frame drawn on an event, when
its completion hands the surface over. The first two set the layer
themselves and no completion follows them, so a mark at the completion
alone would leave the CPU path's logo waiting for good.
`every_frame_on_the_layer_shows_the_window_to_the_splash` holds each
mark, `the_tick_starts_the_splash_only_once_the_window_is_shown` the
tick's gate, and `the_layer_is_set_in_three_places_and_handed_over_in_one`
that the presenter sets the layer nowhere else. On Linux, the first frame
callback, which a server "should avoid signaling ... if the surface is not
visible in any way" ([[source: wayland]]); `Timed::shown` is tested, and
the event loop's call to it is not. **AppKit reports no end to a
window's open animation**: the class's Notifications list has none
([[source: apple-nswindow]]), and `animationBehavior` only lets an app turn
it off ([[source: apple-nswindow-animationbehavior]]). Whether baud's window
animates, and whether these signals come later than the first tick on a
normal launch, is unmeasured: measuring it opens a window on screen.

**How long it stands.** Two full-strength plasmas over the box put a
fourth flash into one second at some pixel unless 0.4 s kept them apart;
every failing pixel is in the box the plasma fills, and ending the emerge
early made it worse ([[source: aj-obs-turn-census]]). The vanish passes
over what the emerge lit under a second before, so it is now drawn as a
trail's repeat pass is (note AL): its bands alone, capped so the amber over
the page and over the lockup makes no transition. It starts at 0.48 s:
from there every frame of the emerge is within 0.10 of the held logo at
every pixel (the last at 0.10 or over is at 0.47 s, light ground, 2x and
at a full-screen 6K frame). Census: general, red and red-encoded at most
3, area 0, on four grounds at 64x20 at 1x 60 Hz, 200x50 at 2x 60 and
120 Hz, and 6120x3330 px at 60 Hz -- margin 0, as before. The vanish's
first frame can draw no glow while its cap is still being scanned
(`FLASH_SCANS` a frame). Measured at one frame on the dark and light
themes, opaque and at a 0xB0 page, with a prompt on screen.

**The choices, from strips** ([[source: aj-obs-turn-strip-dark]],
[[source: aj-obs-turn-strip-light]], [[source: aj-obs-turn-strip-at-end]]):
the vanish at 0.48 s over at the plasma's end, 0.63 s; nothing of the
logo before the window is shown over the emerge's first frame held still;
AppKit's open animation kept over turning it off. Each is the lean. A full
vanish with another seed passed at 0.35 s only, and a seed picked by
search is fitting.

## 11. Snappy: one plasma, swept out, gone in half a second (2026-10-29)

**What the user sees.** Once the window is on screen the logo comes out
of the plasma left to right, as before, and is whole about 0.25 s in; at
0.27 s a quick, even sweep takes it out left to right in 0.15 s, the
block last, while the plasma burns out; what is left of the glow fades by
0.50 s. Something moves on every frame, and nothing waits. It stood whole
about 0.23 s and was gone about 1.1 s after it began; the whole lockup now
stands together for a frame or two, each glyph 0.16-0.27 s, and it is
gone at 0.50 s. Keys, output under the logo and Reduce Motion are as
before; Reduce Motion arriving during the sweep takes it down.

The requests, verbatim (2026-10-29): "so it still holds too long" and "it
feels like it get stuck not snappy".

**What stalled it** ([[source: aj-obs-snappy-measure]]). The vanish's
faint pass was capped from the colours under it, scanned two a frame: its
first frame drew no glow and cost 4.6-7.3 ms of main-thread time (8.3 ms
is a frame at 120 Hz), then the next jumped by up to 0.90 of relative
luminance. Around it the logo stood still from about 0.30 s to 0.48 s and
the block dimmed alone until about 1.1 s, the last owed frames
bit-identical. The pipeline's build (0.05 ms cold) and the window's gate
(at most one tick) were not the cause.

**How it is built.** One plasma, the emerge's, at full strength: no second
pass crosses the box, so there is no cap to scan and no frame waits. The
logo is swept out by an even edge across the lockup (`SWEEP_S`), and the
plasma's records are dimmed in step with the time left after the sweep
(`glow`), so the splash's last frame differs little from the bare page
whenever it falls. `no_splash_frame_waits_on_a_cap_scan`,
`every_frame_the_splash_owes_moves` and
`the_splash_is_gone_within_half_a_second` drive the app's own clock into
the renderer at 60 and 120 Hz; all three failed before the change. Both
GPU arms ran the cap scan; on the instance arm the worst splash frame is
now 0.66 ms of main-thread time and 1.71 ms of GPU at 6K.

**Flashes.** Census: general and red-encoded at most 3, red at most 1, area
0, on four grounds at 64x20 cells 1x 60 Hz, 200x50 2x 60 and 120 Hz and
6120x3330 px at 60 Hz: margin 0, as before, the 3 being the emerge's own.
From 0.47 s on, what is left is within 0.10 of the bare page at every pixel
(`the_plasmas_remainder_fades_into_the_page`).

**The choice, from strips** ([[source: aj-obs-snappy-strip-before]],
[[source: aj-obs-snappy-strip-sweep-dark]],
[[source: aj-obs-snappy-strip-sweep-light]]): the sweep, the lean, over a
wave that erases the logo with the plasma's own trailing edge 0.25 s
behind it, gone 0.57 s ([[source: aj-obs-snappy-strip-wave]]), and over
keeping the faint glow pass with its cap computed ahead, gone 0.63 s with
the 0.23 s hold ([[source: aj-obs-snappy-strip-glow]]). The logo now
leaves in the plasma's fading tail rather than in a pass of its own.

**Not fixed.** On the CPU path, which the Linux driver draws with, the
plasma itself costs 24 ms on its first frame at 2x and 76 ms at 6K, and
frames stay over 8.3 ms through about the eighth at 2x, before and after
this change.
