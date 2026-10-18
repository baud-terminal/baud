# AT. A standard material for the window body

Status: researched 2026-10-17. Nothing is built. Moving the body to a
standard material needs the owner's crate, so it is the owner's call
(section 7). The figures in sections 5 and 6 come from a model. They were
not measured in pixels, and no window was opened.

The owner, 2026-10-17, verbatim: "we should follow apple recommendations for
osx" (note AS).

**What a user sees today.** Text sits on regular Liquid Glass, and the
desktop shows through it, blurred. baud dims the glass at 0.70 in dark mode
and 0.60 in light mode (note AS). Apple's guidance puts Liquid Glass on
controls and a standard material behind app content. With a standard
material:

- the desktop would show through far less, or not at all;
- an inactive window would look subdued, as other Mac windows do;
- under Reduce Transparency the system would probably swap in the
  material's opaque variant (an inference from the catalogs, section 5).

Default text passes 4.5:1 over every modelled wallpaper either way.
Coloured panel text fared worse than default text on glass (note AS,
section 4). So the choice
is about which of Apple's texts baud follows and how much of the desktop
shows through.

**Provenance.**

- **Apple's pages** were fetched on 2026-10-17, the developer pages from the
  documentation JSON endpoint and the WWDC and support pages as HTML. Each
  is pinned by key in `docs/research/sources/MANIFEST.toml` with its
  SHA-256: keys already in the manifest where today's bytes match, and
  group AT for the rest.
- **Two WWDC pages hashed differently today.** Sessions 219 and 310 are the
  same length as note AS's pins but their bytes differ, so today's copies
  are pinned under new keys. Every quote below was checked against today's
  copy.
- **WCAG 2.2 was not re-fetched.** The committed copy is cited: the
  contrast ratio is [[source: wcag22]]'s.
- **Local files.** The material recipes and Apple's app binaries were read
  on macOS 27.0 (26A5425a). baud is cited at `a0822e39`. The owner's crate
  is cited at 1.0.0-beta.5, the version baud links.

## 1. The question

baud's window body is an `NSGlassEffectView`, regular since 2026-10-17, from
the owner's crate macos-liquid-glass. Should it move to an
`NSVisualEffectView` standard material, stay on regular glass, or offer
both with one of them as the default?

## 2. What Apple says

**Liquid Glass belongs to controls, and a standard material to app
backgrounds.** The HIG's Materials page ([[source: apple-hig-materials]]):

- "Don't use Liquid Glass in the content layer." Instead, "use Standard
  materials for elements in the content layer, such as app backgrounds."
- "In contrast to Liquid Glass, the Standard materials help with visual
  differentiation within the content layer."
- "Choose materials and effects based on semantic meaning and recommended
  usage. Avoid selecting a material or effect based on the apparent color it
  imparts", "because system settings can change its appearance and
  behavior."
- "Thicker materials, which are more opaque, can provide better contrast for
  text".

WWDC25 says the same, from today's copies of the pages:

- Session 219 ([[source: apple-wwdc25-219-refetch]]): "Liquid Glass is best
  reserved for the navigation layer". "Avoid putting glass in the content
  layer". Of a table view, "making it Liquid Glass would make it compete
  with other elements and muddy the hierarchy. So keep it in the content
  layer instead".
- Session 310 ([[source: apple-wwdc25-310-refetch]]): "limit your use of
  Liquid Glass to the most important elements in your app".
- Session 356 ([[source: apple-wwdc25-356]]): "controls sit on top of a
  system material, not directly on content."

The owner's crate says it of itself, in mlg-glass.rs, whose bytes are
1.0.0-beta.5's glass.rs ([[source: mlg-glass]]). Liquid Glass is "the
widget/HUD case that guidance permits rather than the general one. If you
are building an ordinary document or list window, the material belongs on
its chrome, not behind its text."

**Which standard material.** `NSVisualEffectView`
([[source: apple-nsvisualeffectview-2026-10-17]]): "Use visual effect views
primarily as background views for your app's content", and "always pick a
material based on its intended use". Each material's own page gives that
use:

| material | Apple's text | see-through |
|---|---|---|
| `.underWindowBackground` | used with behind-window blending "to create a sense of peeking through the back of the window" ([[source: apple-nsvisualeffectview-underwindowbackground]]) | yes |
| `.windowBackground` | "The material for the background of opaque windows." "This material supports Desktop Tinting in Dark Mode." ([[source: apple-nsvisualeffectview-windowbackground]]) | no |
| `.contentBackground` | "The material for the background of opaque content." ([[source: apple-nsvisualeffectview-contentbackground]]) | no |
| `.sidebar` | "The material for the background of window sidebars." ([[source: apple-nsvisualeffectview-sidebar]]) | yes |

Behind-window blending mixes the view "with the contents behind the window,
such as the desktop or other windows"
([[source: apple-nsvisualeffectview-behindwindow]]).

**The inactive window.** A visual effect view's state defaults to
`followsWindowActiveState` ([[source: apple-nsvisualeffectview-state]]): it
appears "active when the window is active, and inactive when it is not"
([[source: apple-nsvisualeffectview-state-enum]]). The HIG's Windows page
says "inactive windows don't use vibrancy (an effect that can pull color
into a window from the content underneath it), which makes them appear
subdued", and "if you use custom implementations, you need to do this work
yourself" ([[source: apple-hig-windows]]). Its "vibrancy" links to the
Materials page. `NSGlassEffectView` is "A view
that embeds its content view in a dynamic glass effect"
([[source: apple-nsglasseffectview-2026-10-17]]). Its page lists no state
property.

**Reduce Transparency and Increase Contrast.**

- On glass, both are automatic. Session 219: Reduce Transparency "makes
  Liquid Glass frostier", and Increase Contrast "makes elements
  predominantly black or white and highlights them with a contrasting
  border", "available automatically whenever you use the new material".
- For a standard material, Apple says only that "system settings can
  change its appearance and behavior" (HIG Materials).
- The Mac User Guide says transparent areas "appear transparent by default"
  and "You can turn these transparent areas a solid gray"
  ([[source: apple-mac-help-display-colors-26]]).
- What an app must do itself is unchanged by the material: "don't use
  semitransparent backgrounds in the user interface. For example, use only
  opaque windows" ([[source: apple-nsworkspace-reduce-transparency]]), and
  "present a high-contrast UI"
  ([[source: apple-nsworkspace-increase-contrast]]).

## 3. What Apple's own text apps put behind their text

These findings come from the binaries, read statically. No app was run.

- **Terminal 2.15** ([[source: macos-terminal-2-15-binary]]) has no
  `NSGlassEffectView` reference.
  - Its see-through window is the profile's background colour at its alpha
    plus a window-server blur, not a material view.
  - The Clear Dark profile ([[source: macos-terminal-clear-dark]]) is alpha
    0.95 with blur 0.5, and Clear Light
    ([[source: macos-terminal-clear-light]]) is alpha 0.93.
  - It creates two `NSVisualEffectView`s, one of them with material
    `.windowBackground`. What that view covers was not traced.
- **TextEdit 1.21** ([[source: macos-textedit-1-21-binary]]) names neither
  view class anywhere in its bundle. Its body is an `NSTextView`.

Neither app puts Liquid Glass behind its text.

## 4. What the shipped materials are

macOS ships each material as a Core Animation archive inside
SystemAppearance.bundle's asset catalogs: DarkAqua.car
([[source: macos-systemappearance-darkaqua-car]]) and Aqua.car
([[source: macos-systemappearance-aqua-car]]). The archives were decoded
with assetutil and compression_tool.

- **WindowBackground and ContentBackground have no backdrop layer.** They
  are an opaque fill: 0.1569 grey in dark for WindowBackground, plus a
  desktop-tint layer at 0.10. So they show nothing of what is behind the
  window.
- **Sidebar** blurs a backdrop and lays a grey over it. Across the blurred
  materials (Sidebar, Menu, FullScreenUI and the generic dark and light
  ones) the blur is radius 30 at scale 0.5, the saturation 1.8 to 2.4, and
  the grey's alpha 0.45 to 0.80 in dark mode.
- **UnderWindowBackground and HUDWindow** were found in none of the 661
  catalogs under /System/Library and /System/Applications. That does not
  prove they are stored nowhere else.

The Sidebar recipe stands in for `.underWindowBackground` in section 6, and
it is a weak stand-in. Sidebar carries a window-server material key and
WindowBackground carries none, which suggests the window server does the
behind-window blur, so the decoded Sidebar recipe may be the within-window
path.

## 5. What changes for baud

**In the owner's crate.**

- The crate has no `NSVisualEffectView` surface in 1.0.0-beta.5 or anywhere
  in its history.
- No new dependency should be needed. objc2-app-kit 0.3.2, which the
  crate already uses, lists `NSVisualEffectView` among its default
  features. Whether the crate keeps default features on was not checked.
- The change is a feature-gated module with an effect surface: material,
  blending mode, state, emphasis and its view.
- `NSVisualEffectView` has no content view, so the surface adds the
  content as a subview.
- Estimated at 100 to 200 lines with its docs and a test. **Not counted.**

**In baud.**

- A `[glass] style` variant.
- A construction branch where the glass is built (`baud-app/src/app.rs`
  7492) and installed (7694-7695).
- `apply_glass` skipping the tint, which a visual effect view does not
  have.
- A test.
- Estimated at 40 to 80 lines. **Not counted.**
- Whether a visual effect view can be installed after the first frame as
  cheaply as the glass is unmeasured.

**The dims and the contrast floor, over each body.** The spread is the
widest in-window spread of the body's luma over the 21 wallpapers, from
section 6's model.

| body | dims 0.70 / 0.60 | the contrast floor | spread |
|---|---|---|---|
| regular glass (today) | hold: set at the bound where the material darkens nothing (note AS, section 3) | at `a0822e39`, reads against the page mixed over the default page at the dim (note AS, section 6); since `a15c6a9c`, reads against the page at its alpha over black to white (record, "The glass floor reads against the whole backdrop") | 41-78 (glass rows, both halves, both bounds) |
| see-through standard (Sidebar stand-in) | hold by the same bound; could come down, unmeasured | same as glass | 10-11 |
| `.windowBackground` / `.contentBackground` | no effect on what shows through: the body is opaque at any dim | its page is what is behind the text | 0 |

**btop's meters.** The floor change that reads text against the whole
backdrop, black to white, covers either glass bound. It landed at
`a15c6a9c`, after this note's base. With it, btop's `#404040` meter ground
on its black page goes to `#bcbcbc` at dark 0.70, so its meters read full.
The figures are in the record entry
"The glass floor reads against the whole backdrop".

A standard material cuts the desktop's spread from 41-78 to about 10 luma,
and an opaque one to none. Over glass anything can be behind the page,
which is why that floor reads against black to white. Over a standard
material the backdrop's range is far narrower, so a floor told the material
would have far less to lift. How many lifts would remain was not counted.
**Modelled.**

**Reduce Transparency and Increase Contrast.**

- baud's own response stays as built at `a0822e39`, whatever the material:
  - Reduce Transparency draws every cell background opaque.
  - Increase Contrast raises the floor's trigger to 3.0 and its target to
    4.5.
- What changes is the system's half. Glass frosts itself. A visual effect
  view turns opaque: every material ships an `-Opaque` variant in the
  catalogs. Which setting selects that variant is an inference; Apple's
  text does not say.

## 6. Modelled renders

**What the renders show.** baud's own CPU-rendered cells for btop and git
log, composited over each body at the shipped dims. Every panel is labelled
MODEL. Each sheet is 2x2:

- top left: glass;
- top right: the see-through standard;
- bottom left: `.windowBackground`;
- bottom right: `.contentBackground`.

**How each body is modelled.**

| body | modelled as |
|---|---|
| glass | note AS's clear fit (k = 0.695), standing in for regular; no data separates the two |
| see-through standard | the decoded Sidebar recipe |
| `.windowBackground`, `.contentBackground` | their decoded recipes, the desktop tint taken as the wallpaper's mean |

Blending is in encoded sRGB, with the blur radius taken as a Gaussian sigma
of 30 pt.

![btop, dark at 0.70, over Tahoe Evening](../sources/at-observed/sheet-btop-dark-tahoe-evening.png)

[[source: at-obs-png-sheet-btop-dark-tahoe-evening]]

**btop, dark at 0.70, over Tahoe Evening.** The wallpaper's blue shows in
the top half only. The bottom row is flat grey.

![git log, light at 0.60, over Tahoe Evening](../sources/at-observed/sheet-git-log-light-tahoe-evening.png)

[[source: at-obs-png-sheet-git-log-light-tahoe-evening]]

**git log, light at 0.60, over Tahoe Evening.** On glass the wallpaper's
clouds show behind the text. The standard bodies read as near-white pages.

![btop, dark at 0.70, over a white wallpaper](../sources/at-observed/sheet-btop-dark-white.png)

[[source: at-obs-png-sheet-btop-dark-white]]

**btop, dark at 0.70, over a white wallpaper.** This is the dark half's
worst case. The glass panel is the lightest of the four.

**Default text, modelled contrast ratio**, worst window position / median.
The last column is the widest in-window body-luma spread over all 21
wallpapers:

| body / half | Tahoe Evening | Sonoma | iMac Yellow | white | black | 21 pass 4.5:1 at worst | spread |
|---|---|---|---|---|---|---|---|
| glass, clear fit, dark | 9.99/12.29 | 8.89/10.71 | 8.27/8.90 | 8.04 | 13.75 | 21 | 40.7 |
| glass, clear fit, light | 4.83/5.86 | 5.80/7.02 | 7.63/8.55 | 9.41 | 4.68 | 21 | 54.2 |
| glass, k = 1, dark | 8.21/11.52 | 6.84/9.16 | 6.13/6.85 | 5.88 | 13.75 | 21 | 58.5 |
| glass, k = 1, light | 4.89/6.43 | 6.34/8.26 | 9.24/10.74 | 12.17 | 4.68 | 21 | 78.0 |
| see-through standard, dark | 11.66/12.23 | 11.05/11.70 | 10.71/10.94 | 10.52 | 12.77 | 21 | 11.1 |
| see-through standard, light | 11.33/11.75 | 11.88/12.21 | 12.57/12.60 | 12.78 | 11.10 | 21 | 9.7 |
| `.windowBackground`, dark | 12.44 | 12.20 | 11.90 | 11.74 | 12.70 | 21 | 0.0 |
| `.windowBackground`, light | 12.91 | 13.06 | 13.24 | 13.34 | 12.75 | 21 | 0.0 |
| `.contentBackground`, dark | 12.76 | 12.53 | 12.25 | 12.09 | 13.00 | 21 | 0.0 |
| `.contentBackground`, light | 13.75 | 13.75 | 13.75 | 13.75 | 13.75 | 21 | 0.0 |

**Contrast does not separate the options**: every body passes on every
wallpaper. What separates them is how much of the desktop shows through,
and which of Apple's texts each follows.

**What only an on-screen look can settle:**

- regular glass against a standard material, in real pixels;
- what `.underWindowBackground` really looks like;
- the inactive state;
- both display settings on each material;
- whether the dims are still wanted over a standard material.

## 7. Jev, and the owner's call

**How Jev was asked.** Jev (jev-1.13.0) was asked twice, each time in both
orders. All four answers were uncached: `decisions.jsonl` 730-733. The
requests carried:

- the three contact sheets;
- Apple's quotes;
- the owner's words;
- a statement that the evidence is uneven between the arms;
- a statement that even a 0.9 answer needs the owner's crate.

| ask | pick (confidence) | stay glass | move, under-window | move, window background | both, standard default | both, glass default |
|---|---|---|---|---|---|---|
| 1, original order (730) | move, window background (0.27) | 0.02 | 0.17 | 0.42 | 0.38 | 0.01 |
| 1, reversed (731) | move, window background (0.70) | 0.02 | 0.19 | 0.76 | 0.03 | 0.00 |
| 2, original order (732) | move, under-window (0.49) | 0.01 | 0.59 | 0.04 | 0.36 | 0.00 |
| 2, reversed (733) | move, under-window (0.81) | 0.01 | 0.85 | 0.04 | 0.10 | 0.00 |

**What round 2 added:**

- the search of all 661 catalogs;
- that `.windowBackground` means an opaque window, where the dims stop
  mattering;
- the owner's decision 9 in note Y verbatim (opaque while Reduce
  Transparency is on, "and only then");
- Apple's rule to pick a material by its intended use;
- that Apple's Terminal ships translucent.

**Framing disclosure.** Round 2's added text described what each move
means, and the pick swung from the window background to the under-window
material. So which standard material is not settled: it depends on how the
question is put. Every ask did agree on one result:

- staying on glass scored 0.01-0.02;
- glass as the default with a standard material as an option scored
  0.00-0.01;
- the three options with a standard default together scored 0.97 or more.

**Nothing reached 0.9, so Jev decided nothing.** After the re-ask, Jev's
lean is to move to `.underWindowBackground` (0.49/0.81, decisions
732/733). The proposal below keeps that material as the default and adds
glass as an opt-in, which Jev scored 0.36/0.10. It goes to the owner
because it needs the owner's crate.

**The research's proposal**, which is not Jev's pick, since Jev never ranked
it first:

- the default is `.underWindowBackground` with behind-window blending and
  the state that follows the window;
- regular glass stays as an opt-in `[glass] style`;
- the dims stay until an on-screen look says otherwise.

If the owner wants a body that does not show the desktop, `.windowBackground`
is Apple's "material for the background of opaque windows"
([[source: apple-nsvisualeffectview-windowbackground]]).

**It is the owner's call.** Every option but staying on glass needs a new
surface in the owner's crate, so it is the owner's call even at 0.9 (note
AS, section 6). Nothing is built before the owner sees a standard-material
window on screen.
