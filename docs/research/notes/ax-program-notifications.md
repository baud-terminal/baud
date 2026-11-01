# AX. Notifications and progress from programs

**Status: 2026-10-21.** A program can post a desktop notification and
show a progress bar: `OSC 9` (iTerm2), `OSC 777 ; notify` (Terminology
and VTE), kitty's `OSC 99`, ConEmu's `OSC 9 ; 4`, and `OSC 1337 ;
RequestAttention`. This note pins each text, says how the terminals
that implement them disagree, records what was decided, and says what
each platform lets a terminal do with them.

## 1. The texts

| sequence | governing text | what it says |
|---|---|---|
| `OSC 9 ; Pt` | `[[source: iterm2-escape-codes]]`, "Post a notification" | "OSC 9 ; [Message content goes here] ST". `[[source: iterm2-feature-reporting]]`: the text "will be displayed to the user outside the normal UI so that even if they are using another application it may still be visible". |
| `OSC 9 ; 4 ; st ; pr` | `[[source: conemu-ansi-escape-codes]]`, "ConEmu specific OSC" | "0: remove progress. ... 1: set progress value to pr (number, 0-100). ... 2: set error state in progress on Windows 7 taskbar, pr is optional. ... 3: set indeterminate state. ... 4: set paused state, pr is optional." |
| | `[[source: iterm2-escape-codes]]` 3.7 | the same states; "To stop the progress bar, you can also use the abbreviated form: OSC 9 ; 4 ST"; state 4 without `pr`: "the current percentage is kept and simply recolored". 3.6 and `[[source: wt-progress-bar]]` call 4 "Warning". |
| `OSC 9 ; 1`-`12` | `[[source: conemu-ansi-escape-codes]]` | sleep, a message box, the tab title, wait for a key, GuiMacro, run a process, an environment variable, the directory, xterm emulation, a comment, the prompt start. |
| `OSC 777 ; notify ; S ; B` | `[[source: vte-notify-patch]]` | "OSC 777 ; notify ; SUMMARY ; BODY" and "OSC 777 ; notify ; SUMMARY", "Based on Enlightenment's Terminology". rxvt-unicode itself only routes `OSC 777` to perl: "Call the perl extension with the given string, which should be of the form extension;parameters" (`[[source: rxvt-7-pod]]`, `[[source: urxvt-pm]]`), and 9.31 ships no `notify` extension. |
| `OSC 99 ; meta ; payload` | `[[source: kitty-desktop-notifications]]` | the whole protocol: chunking by `i` and `d`, `e=1` base64, `p` title, body, close, icon, `?`, alive, buttons; `a`, `c`, `f`, `g`, `n`, `o`, `s`, `t`, `u`, `w`; the query and its answer; the activation, button and close reports; "Terminals **must** sanitize ids". |
| `OSC 1337 ; RequestAttention=` | `[[source: iterm2-escape-codes]]`, "Attention" | "yes to request attention by bouncing the dock icon indefinitely, once to bounce it a single time, or no to cancel a previous request. If it is fireworks then fireworks explode at the cursor's location." |

The other ~30 `OSC 1337` subcommands are listed with one row each in
`docs/vt-coverage.md`, in scope and unbuilt.

## 2. How terminals tell OSC 9 text from ConEmu's forms

| body | iTerm2 3.7 | Windows Terminal | kitty 0.49.2 | ghostty 1.3.1 | baud |
|---|---|---|---|---|---|
| `Hello` | notification | ignored | notification | notification | notification |
| `4;1;50` | progress | progress | progress | progress | progress |
| `4` | clears progress | progress, state 0 | notification "4" | notification "4" | clears |
| `4;9` | -- | dropped (state > 4) | dropped, logged | notification "4;9" | dropped, counted |
| `9;C:/dir` | notification | directory | notification | directory | refused, counted |
| `2;text` | notification | ignored | notification | message box | refused, counted |

Sources: `[[source: wt-adapt-dispatch]]` (`DoConEmuAction`),
`[[source: kitty-window-py-0492]]` (`desktop_notify`: "This is probably
the ConEmu progress reporting conflicting implementation"), `[[source:
ghostty-osc9]]`. Decided: ConEmu's forms first, bare `9;4` clearing, the rest text; `9;4;9` dropped. The argument that moved it: Windows Terminal's own docs have
shells send `9;9` at every prompt, and reading that as text posts a
notification at every prompt.

## 3. The rest of the forks

Each weighed against two rules: a protocol is built whole, and a setting is "on" or "off":

- **The `OSC 777` body** is everything after the title's `;`, and a
  title alone is a notification. VTE's tokeniser
  stops at the next `;`, yet its own `vte.sh` sends a command line there;
  ghostty refuses a title alone, which VTE's text allows.
- **Focus**: `OSC 99` follows `o`, whose default is `always` ("always
  honors the request"), so it shows while focused unless the program
  says otherwise; `OSC 9` and `OSC 777`, whose texts say nothing, show
  only while the window is not focused.
- **Icons**: `n` names mapped where the platform has named icons, `p=icon`
  data cached under `g` with a bounded cache.
- **macOS permission**: explicit authorization, asked when a program
  first sends a notification with the setting on. Provisional delivery would put a program's "build finished"
  only in Notification Center's history.
- **The flood bound**: a bucket of four, refilled one a second.
- **Stale progress**: only the texts clear it -- state 0, bare `9;4`,
  `RIS`; no timeout (kitty's 60 s is its practice).
- **`OSC 1337`**: `RequestAttention` built with notifications, every
  other subcommand its own row.
- **`OSC 99`'s value characters**: the text's set with `=`, which the
  text's own base64 values need and kitty's own client sends (`[[source: kitten-notify-go]]`), over the set exactly and over no check. The `?` of the text's own `p=?` is admitted too, which was not
  weighed: the text writes it. A value with any other byte makes the
  command malformed.
- **macOS's `p=alive`**: Notification Center's delivered list, asked every
  few seconds while any is shown, over only baud's own removals, as kitty's own client reads it.

## 4. The platforms

**macOS**, from Apple's pages and the SDK headers only.
`UNUserNotificationCenter` is "The central object for managing
notification-related activities for your app or app extension"
(`[[source: apple-unusernotificationcenter]]`); `current()` "Returns your
app's notification center" (`[[source:
apple-unusernotificationcenter-current]]`); "you must obtain permission
to use them", and "The first time your app makes this authorization
request, the system prompts the person" (`[[source:
apple-asking-permission-notifications]]`, `[[source:
apple-request-authorization]]`). **Neither the pages nor the headers say
what a process with no bundle gets**, nor whether an ad-hoc signature is
enough (`[[source: macos-sdk-unusernotificationcenter-h]]`), and no probe
was run to find out: each would put a prompt on the screen.
`removeDeliveredNotificationsWithIdentifiers` closes one, and the
delegate's `didReceiveNotificationResponse` is the click; nothing says
when the user dismisses one, so closes are untracked there, which kitty's
text has an answer for (`p=close ; untracked`). The Dock tile: "Draw the
tile content yourself ... you must provide a custom content view"
(`[[source: apple-nsdocktile]]`, `[[source: macos-sdk-nsdocktile-h]]`),
which is where a progress bar goes. `requestUserAttention`: "Activating
the app cancels the user attention request" (`[[source:
apple-request-user-attention]]`).

**Linux.** `org.freedesktop.Notifications` on the session bus (`[[source:
freedesktop-notification-spec]]`): `Notify(app_name, replaces_id,
app_icon, summary, body, actions, hints, expire_timeout)`, whose reply is
the id; `replaces_id` is kitty's update by id; `ActionInvoked` the click
and the buttons ("The default action ... should have a key named
default"); `NotificationClosed` the close; hints `urgency` (a byte, 0-2),
`sound-name`, `suppress-sound`, `image-data`. Body markup is the server's
`body-markup` capability: "If marked up text is sent to a server that does not give this cap, the
markup will show through as regular text" -- so text a program sent must
be escaped for a server that has it. Progress: no freedesktop
standard; the one docks read is `com.canonical.Unity.LauncherEntry`'s
`Update(app_uri, {progress, progress-visible})` (`[[source:
libunity-launcher-entry]]`), matched to an installed desktop file by its
`application://` URI. baud's Wayland app id is `org.baud.terminal` and no
desktop file ships, so a dock has nothing to match the signal to until
one does.

## 5. What baud does

- **macOS** (`baud-present-macos/src/notify.rs`, `baud-app/src/notify.rs`):
  the center is opened only when the main bundle has an identifier; run
  from `cargo run`, programs' notifications are refused and the log says
  why. Permission is asked at the first notification, and what arrives
  before the answer waits for it. A post's identifier is its token's, so a
  kitty update by id replaces it; buttons are a category's actions
  `button-1` and on; `u=0` is passive and the rest active; `s=silent` has
  no sound; an image is attached from a file of its own. Closes are not
  told, so a program that asks is answered `untracked`, and `p=alive`
  follows `getDeliveredNotificationsWithCompletionHandler`, asked every
  two seconds while any is shown; one closed by `p=close`, by its `w`, or
  posted while permission was refused is forgotten at once. `w` is a
  removal scheduled on the main queue. A refusal of permission tells the
  core nothing is delivered. Progress is a bar drawn on the Dock tile's
  own view under the icon; `RequestAttention` is
  `requestUserAttention`, critical for `yes`, informational for `once`.
  **Open**: a program that asked is told `p=close ; untracked` when the
  post is made, before permission is known; if permission is then
  refused, it was told of a notification that never showed. And the
  delivered-list ask has no deadline: if
  `getDeliveredNotificationsWithCompletionHandler` never answers, no
  further ask is made and `p=alive` keeps naming what it last knew.
- **Linux** (`baud-app/src/bus.rs`, `baud-app/src/linux.rs`): `Notify`
  with `replaces_id` for an update, a `default` action where a click is
  reported and the buttons numbered from 1, `urgency`, `sound-name` or
  `suppress-sound`, `category`, and a PNG of at most 1024 pixels a side
  as `image-data`; JPEG and GIF data, which the text also names, are
  refused and counted until a decoder exists, as is
  a larger PNG, and the log says so once; the body
  escaped where the server lists `body-markup`; an icon name with a `/`
  or a `:` is not sent, since `app_icon` also takes a file URI;
  `GetCapabilities` decides whether `report` and `buttons` are claimed, and until it
  answers -- or when it fails, or the bus goes -- the core is told nothing
  is delivered. `w` is `expire_timeout` and a
  `CloseNotification` at the deadline. Progress and `RequestAttention` are
  the launcher's `progress` and `urgent`.
- **Neither** posts while the setting is off, nor more than four at once
  and then one a second; what the bound drops is logged once a burst,
  each line counting every drop since the one before, a burst ending
  when the bucket has filled again.

## 6. Several windows, one center and one Dock tile

Since Command-N, the macOS app runs several windows in one process, each
with its own terminal, and the notification center and the Dock tile are
still one each.

**Notifications.** Each terminal numbers its notifications from the first,
so two windows' first posts had one identifier and the second replaced the
first in Notification Center; and each window set its own delegate on the
app's one center, whose delegate property is `weak` (`[[source:
macos-sdk-unusernotificationcenter-h]]` 42), so the newest window heard
every click and, once it closed, no window held the delegate the center
named. Now an identifier carries
the window's key, every window's center shares the process's one
delegate, and a click goes to the window whose key its identifier carries,
or is dropped once that window has closed.

A window takes its clicks on its display link's tick, and the tick pauses
while the window is covered or minimised. So a click on a hidden window's
notification raises that window and reaches its program only once the
window is uncovered. Open.

**Progress.** The Dock tile showed whichever window's progress changed
last, and a window closed while showing progress left its bar on the tile.
The texts: `OSC 9;4` is ConEmu's, made for the Windows taskbar ("set error
state in progress on Windows 7 taskbar", `[[source:
conemu-ansi-escape-codes]]`), and the taskbar documents what one button
shows for several windows: "The taskbar button can show a progress
indicator for only one window at a time ... the taskbar button chooses its
progress display based on state priority", error, paused, normal,
indeterminate, and "In the case of a priority collision between two windows
that are broadcasting determinate progress, the window with the least
progress is used" (`[[source: ms-itaskbarlist3-setprogressstate]]`).
Apple's Dock tile has no progress of its own (`[[source: apple-nsdocktile]]`).

| terminal | several windows' progress |
|---|---|
| Windows Terminal | every tab combined by the taskbar's priority, "lowest priority wins" (`TaskbarState::Priority`, `[[source: wt-6676c6f-taskbarstate]]`; `TerminalPage::TaskbarState`, `[[source: wt-terminalpage]]` 4233-4261) |
| kitty | the Dock bar is the mean percent of every window with one, else indeterminate, else none; an error alone shows nothing (`update_progress_in_dock()`, `[[source: kitty-1e1486e-boss]]` 4075-4093) |
| Ghostty | each surface's bar in its own view; the Dock tile only for the bell badge (`[[source: ghostty-d9840f3-surface-progress-bar]]`) |
| iTerm2 | each session's bar in its view and its tab; the Dock tile only for badges (`[[source: iterm2-269f725-tab-bar-control-view]]`) |
| Terminal.app | closed source; no progress support found; not weighed |

Decided: the tile shows the most urgent window's bar by the taskbar's
group rule, over kitty's mean and over the key window's own, and a
window's close takes its bar with it (`notify::DockBars`). A failed build in a window behind the others shows
red on the Dock until that window clears it or closes.

Among error or paused bars, the tile also shows the one with the least
progress. That is baud's own extension: Microsoft's text gives the
least-progress rule only for determinate bars.

**Linux** is not combined: each window is its own process and sends the
launcher's `progress` for itself under the one application id, and what a
dock makes of several is the dock's. Not built.

## 7. The fireworks

`RequestAttention=fireworks` is the request's one drawn form: "If it is
fireworks then fireworks explode at the cursor's location"
(`[[source: iterm2-escape-codes]]`). The page says no more, so the look is
iTerm2's, read at 964a75d. PTYSession's `screenRequestAttention:`
(`[[source: iterm2-964a75d-ptysession]]`) calls PTYTextView's
`showFireworks` (`[[source: iterm2-964a75d-ptytextview]]`), which opens a
transparent window over the screens holding iTermFindCursorView's star
view (`[[source: iterm2-964a75d-findcursorview]]`): a `CAEmitterLayer` in
additive mode (`[[source: macos-sdk-caemitterlayer-h]]`) at the cursor's
centre, one cell born four a second, each throwing three 200-point stars,
yellow, red and white (`[[source: iterm2-964a75d-findcursorcell1]]`,
`[[source: iterm2-964a75d-findcursorcell2]]`,
`[[source: iterm2-964a75d-findcursorcell3]]`), fast at 1000 points a
second and slow at 100, growing, spinning and fading over a second
(`[[source: macos-sdk-caemittercell-h]]`), a 20-point hole round the
cursor. The emitter follows the cursor on every draw; the window goes 1 s
after the last request, its alpha animated over the default 1/4 s
(`[[source: macos-sdk-catransaction-h]]`), linearly
(`[[source: macos-sdk-caanimation-h]]`). Nothing in the three files
checks Reduce Motion.

**Measured against WCAG 2.3.1** by baud's census
(`baud-bench/tests/fireworks_flashes.rs`), drawn with every iTerm2 number
the show flashes more than three times a second over 28,464 square
points of a field on the verdict's reading and 32,258 on the chained
one, past the 21,824 the general flash threshold allows (1x, light page,
120 Hz; `bauds_fireworks_pass_over_a_full_window`). The slow stars at 40
points a second and half as many fast ones pass, at 9,665 at worst on
the dark and light pages (55.7% under, chained, light page, 120 Hz) and
9,713 on the #1A0000 ground (55.5% under, chained, 120 Hz), and 9,597 at
2x; a show held
four seconds by repeated requests at 16,602 (23.9% under, chained, dark
page, 120 Hz), and two shows, the second begun in the first's fade,
under that.

**Decided**, 2026-11-01: the stars are drawn inside baud's own window,
since the Linux driver cannot place an overlay and both systems draw
alike; under Reduce Motion the show's frame at 0.5 s is held still and
faded out over 1.25 s, the fade Apple's guidance puts in place of motion
(`[[source: apple-hig-accessibility]]`). **Taken, and open**: the look
above rather than the same with the stars not spinning; and each star
drawn over the frame rather than added to the others first, since the
additive form needs a summing pass on every path, and by construction
the two differ only where stars overlap: one star alone gives the same
pixel either way.

**Built** (`baud-render/src/fireworks.rs`, `baud-app/src/fireworks.rs`):
the core hands the request's cell to the embedder (`Term::fireworks`);
the shows hold the effect's slot after the launch splash, a trail lit
meanwhile held until it lands; a request while a show holds restarts its
hold, one while it fades starts a second. Under Reduce Motion a request
while a still show is up starts nothing. The star's distance function
folds the point into one wedge and measures it to one edge, the form of
Inigo Quilez's sdStar5() among his 2D distance functions, the geometry
spelled over again rather than the code copied, as
`docs/research/notes/w/spec-ufo.json` records for the ufo effect's.

**Not built**: iTerm2 also scrolls the cursor into view when the request
comes; baud leaves the window where the user put it and draws at the
cursor's cell where the window shows it.
