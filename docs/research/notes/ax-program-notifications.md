# AX. Notifications and progress from programs

**Status: 2026-10-21.** A program can post a desktop notification and
show a progress bar: `OSC 9` (iTerm2), `OSC 777 ; notify` (Terminology
and VTE), kitty's `OSC 99`, ConEmu's `OSC 9 ; 4`, and `OSC 1337 ;
RequestAttention`. This note pins each text, says how the terminals
that implement them disagree, records what Jev decided, and says what
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
ghostty-osc9]]`. Jev: ConEmu's forms first, bare `9;4` clearing, the
rest text -- lean 0.83 and 0.91; `9;4;9` dropped -- 0.98 and 0.96,
decided. The argument that moved it: Windows Terminal's own docs have
shells send `9;9` at every prompt, and reading that as text posts a
notification at every prompt.

## 3. The rest of the forks

All asked of Jev in both orders, uncached, with the owner's standing
rule quoted ("full VT420-class terminal. we need to support such
protocols fully, always!") and the settings rule ("i would say on or
off, not answer, keep it simple"):

- **The `OSC 777` body** is everything after the title's `;`, and a
  title alone is a notification: lean 0.85 and 0.67. VTE's tokeniser
  stops at the next `;`, yet its own `vte.sh` sends a command line there;
  ghostty refuses a title alone, which VTE's text allows.
- **Focus**: `OSC 99` follows `o`, whose default is `always` ("always
  honors the request"), so it shows while focused unless the program
  says otherwise; `OSC 9` and `OSC 777`, whose texts say nothing, show
  only while the window is not focused: 0.98 and 0.97, decided.
- **Icons**: `n` names mapped where the platform has named icons, `p=icon`
  data cached under `g` with a bounded cache: 0.95 and 0.97, decided.
- **macOS permission**: explicit authorization, asked when a program
  first sends a notification with the setting on: 0.94 and 0.98,
  decided. Provisional delivery would put a program's "build finished"
  only in Notification Center's history.
- **The flood bound**: a bucket of four, refilled one a second: lean 0.86
  and 0.91.
- **Stale progress**: only the texts clear it -- state 0, bare `9;4`,
  `RIS`; no timeout (kitty's 60 s is its practice): 0.98 and 0.98,
  decided.
- **`OSC 1337`**: `RequestAttention` built with notifications, every
  other subcommand its own row: 0.97 and 0.92, decided.
- **`OSC 99`'s value characters**: the text's set with `=`, which the
  text's own base64 values need and kitty's own client sends (`[[source:
  kitten-notify-go]]`): lean 0.30 and 0.48, over the set exactly and over
  no check. The `?` of the text's own `p=?` is admitted too, which was not
  put to Jev: the text writes it. A value with any other byte makes the
  command malformed.
- **macOS's `p=alive`**: Notification Center's delivered list, asked every
  few seconds while any is shown, over only baud's own removals: lean 0.82
  and 0.90, after a re-ask with kitty's own client in the context.

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
  refused and counted until a decoder exists (Jev, 0.95 and 0.97), as is
  a larger PNG, and the log says so once; the body
  escaped where the server lists `body-markup`; an icon name with a `/`
  or a `:` is not sent, since `app_icon` also takes a file URI;
  `GetCapabilities` decides whether `report` and `buttons` are claimed, and until it
  answers -- or when it fails, or the bus goes -- the core is told nothing
  is delivered. `w` is `expire_timeout` and a
  `CloseNotification` at the deadline. Progress and `RequestAttention` are
  the launcher's `progress` and `urgent`.
- **Neither** posts while the setting is off, nor more than four at once
  and then one a second; what the bound drops is logged once a burst, a
  burst ending when the bucket has filled again.
