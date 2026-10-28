# AI. Where a Dock-launched baud's warnings go

**Status: research, 2026-10-09.** Its sources are in
`docs/research/sources/MANIFEST.toml`, group `AI`: 91 entries, 49 `retrieved`
and 42 `external`. The probes' sources and raw outputs are the `ai-obs-*`
entries under `docs/research/sources/ai-observed/`, redacted: the user name,
the scratch path and process ids are replaced, and `PATH` was elided at
capture; times keep their zone (MDT). The unredacted originals and the fetched
copies of external sources stay untracked under `.claude/research/logging/`.
[[source: ai-obs-index]] says what each holds. **Not put to Jev** when written;
Jev is asked before the questions go to the owner. Jev was asked on
2026-10-09 (decision log 353-354): question 1 is decided (§7), and question
2's C shim cleared 0.9 both times but is held as conditional.

Evidence tags:
- **M**: measured on this Mac (macOS 27.0 build 26A5425a, Xcode 27.0 27A266a,
  on AC power), with the raw output saved as an `ai-obs-*` entry, cited where
  it is used.
- **C**: read from baud's code at `ddf6d28`. Since then `baud-app/src/app.rs`
  has changed (63596fd): at ccb8aaa its lines from 2180 on are shifted, so
  app.rs:3978 is 4155 and the cited lines from 5102 on are +186. Every other
  cited file is unchanged at ccb8aaa. The files are
  `baud-app/` (`src/`, `build.rs`, `Cargo.toml`), `baud-domain/src/image.rs`,
  `baud-domain/tests/image_store.rs`, `baud-core/tests/image_anchor.rs`,
  `baud-core/tests/sixel.rs`, `baud-bench/benches/ingest2.rs`,
  `baud-sfte-difftest/build.rs`, `baud-wayland/src/sys.rs` and `packaging/`.
  The record was read at HEAD.
- **UNVERIFIED**: neither quoted nor measured.

## In plain words

- **A launched baud's log lines are lost.** baud logs to stderr. A baud
  launched through LaunchServices has its stderr connected to `/dev/null`, so
  every line goes nowhere, at every level.
  - Measured for a launch through `open(1)`. That launchd did the connecting is
    inferred from the parent being pid 1 and from launchd.plist(5).
  - A click on the Dock and a double-click in Finder were not probed.
- **`BAUD_LOG` cannot be set from a shell for such a launch.** A Dock-launched
  process is expected to get launchd's environment, not a shell's. That is not
  probed; an `open` run under `env -i` imitated it. The texts read here offer
  two ways to set a variable for it: `launchctl setenv`, a legacy subcommand,
  and the bundle's `LSEnvironment`. A config key would be baud's own.
- **The premise needs one correction.** The Sixel refusal line
  (`describe_refusals`) is already `warn`. The `info` line is the image
  *budget* (`describe_images`). `placements_refused` is never said, which
  the record already records.
- **Apple's guidance for an app's messages is the unified log (`os_log`).** It
  "supersedes" syslog, and it stores "in memory and on disk, rather than
  writing that data to a text-based log file".
  - Its C interface is a macro over a compiler builtin, so Rust cannot call it
    as documented.
  - The route that stays on the documented surface is a few lines of C that
    expand the macro. ghostty uses exactly such a shim, and kitty, partly
    written in C, calls the macro directly.
  - `syslog(3)` also reaches the unified log with no C. But it is the API
    Apple says is superseded, and its lines carry no subsystem.
- **Linux.**
  - journald documents a socket protocol that baud could speak over `sendmsg`.
  - GNOME Shell already connects a launched app's stderr to the journal. That
    is one desktop's practice, not a standard.
  - The XDG spec says `$XDG_STATE_HOME` "may contain" logs.
- **Prior art.**
  - ghostty and kitty log to the unified log on macOS.
  - alacritty and wezterm write a log file: alacritty in the temp dir, wezterm
    in `~/.local/share/wezterm` on macOS.
  - foot uses syslog, in server mode only.
  - iTerm2 does both. Its error lines go to `NSLog` and to a rotating file
    written on every call. Its opt-in debug log goes to `/tmp` and,
    optionally, to the unified log, but without the message text.
- The owner's questions are in §7.

## 1. baud today

### 1.1 The log module

`baud-app/src/log.rs` (C) has five macros: `err!`, `warn!`, `info!`, `debug!`
and `verbose!`.
- A threshold gates them. It is read once from `BAUD_LOG` in `log::init`,
  which `main` calls second, after `host::startup_begin`. The default is `warn`.
- Each macro is an `eprintln!` behind `log::enabled`. The line carries no
  timestamp and no level tag.
- [[source: rust-std-eprintln]]: `eprintln!` "Panics if writing to
  `io::stderr` fails. Writing to non-blocking stderr can cause an error, which
  will lead this macro to panic." A write to `/dev/null` does not fail, which
  is why a Dock launch is silent rather than crashing.
- Nothing in the shipping binary installs a panic hook (M,
  [[source: ai-obs-set-hook-sweep]]). That was checked by a grep of the
  workspace crates and of the 25 registry crates in baud-app's macOS
  dependency tree, with std's own `set_hook` found as the control. The hits:
  - test code: baud-font's and baud-render's tests, objc2's
    `tests/track_caller.rs`, and zerocopy's `src/layout.rs:1401-1412`, inside
    `#[cfg(test)] mod tests`;
  - proc-macro2's `src/detection.rs`, which swaps a null hook in and the
    original back, and runs inside the compiler (proc-macro2 is reached only
    through the proc-macro crate zerocopy-derive).
- So the standard hook runs, and [[source: rust-std-panicking]] says: "The
  default hook, which is registered at startup, prints a message to standard
  error".

### 1.2 Every message

baud-app has 50 macro call sites (C, grep):

| File | Sites |
|---|---|
| app.rs | 26 |
| linux.rs | 19 |
| pty.rs | 2 |
| config.rs | 1 |
| host.rs | 1 |
| main.rs | 1 |

By level that is `err` 1, `warn` 29, `info` 20, `debug` and `verbose` 0. There
are also 12 `eprintln!` sites outside `log.rs`. The macOS driver's sites are
below. "Carries" names anything in the line that is a path, a link or a value
taken from the user's files.

| Where (C) | Level | What it says | Carries |
|---|---|---|---|
| app.rs:167 | warn | `BAUD_GPU=… is not 0, 1 or cells; choosing automatically` | an env value |
| app.rs:598 | info | `cache: …; grid pitch N cells, N B a row` | -- |
| app.rs:650-651 | info | `config: <path>` / `config: none found; the built-in stack` | a home path |
| app.rs:654 | info | `face: <path>` | a font path |
| app.rs:656 | info | `images: N MiB pixels`: **the budget line** (`host::describe_images`); `, N KiB body` until 2026-10-14, when the body scratch went | sizes |
| host.rs:210 | warn | `font <path>: N of its N CBLC sizes are not read: …` (`bitmap_refusals`) | a font path |
| config.rs:487 | warn | one sentence per config key it could not act on | config text |
| app.rs:670 | warn | the palette loader's sentences (`config: [palette] …`) | theme names, paths |
| app.rs:770, 5102 | info | `motion: …; trail: …` | -- |
| app.rs:797, 806, 863, 1231 | warn | renderer fallbacks, e.g. `no Metal device or shaders; drawing on the CPU` | -- |
| app.rs:874, 1237 | info | `renderer: …`, `presenting through …, memory …` | -- |
| app.rs:887-893 | **none: bare `eprintln!`** | `trace: off (set BAUD_TRACE=<prefix> to record the session)` on every launch without `BAUD_TRACE`, or the trace-file lines | a path prefix |
| pty.rs:230 | warn | `pty: the master stays blocking, so a full queue waits: <error>` | -- |
| app.rs:1643, 1654 | info | the five-second frame report | -- |
| app.rs:1670 | info | `links: N opened, N revealed, N refused` | counts |
| **app.rs:3978** | **warn** | the arena and slot sentences, once a pump (`host::describe_refusals`); until 2026-10-14 also `images: a Sixel refused for its body, the last N bytes (N KiB) against a N-byte (N KiB) body scratch; [images] body_kb raises it`, gone with the scratch | sizes |
| app.rs:5154 | info | `caret: …, switch …, read in N us` | -- |
| app.rs:5162 | warn | `baud: [cursor] blink = true overrides the system's non-blinking caret; …`, once | -- |
| app.rs:5227 | warn | `link refused (gone)` | -- |
| app.rs:5241, 5259 | warn | `link refused (…): <uri>` | **a link**: its first 80 bytes, `escape_ascii` |
| app.rs:5251, 5255 | info | `link opened (<scheme>): <uri>`, `link revealed: <uri>` | **a link** |
| pty.rs:911 | warn | `pty: poll cannot watch the master; it blocks again`, said **on the reader thread** | -- |

**The rest of the `eprintln!` sites.** Each is gated by its own variable, and
`BAUD_LOG` gates none of them:
- `BAUD_DEBUG_GEOMETRY`: app.rs:1242.
- `BAUD_DEBUG_RESIZE`: app.rs:5628, 5639 and 5674.
- `BAUD_KEYLAT`: app.rs:1666 and 1735, and latency.rs:548.
- `BAUD_STARTUP`: host.rs:329 and 347.

These are developer instruments run from a shell, and this note leaves them
alone. Separately, a face that cannot be mapped or parsed panics with
`font <path>: …` (host.rs:197, 208). By §1.1 that message goes to stderr.

**The Linux driver (`linux.rs`, C).**
- Its startup `info` lines include macOS's config, face, budget and cache
  lines (105-121). It does not have the motion, renderer, presenting or caret
  lines.
- It writes the refusal line at 368 (`warn`).
- `err` "the compositor closed the connection" (265).
- `warn` lines for Wayland and `poll` errors, a dropped key (310) and a
  truncated window (408).
- At 109 it logs `warn` "no font: none of the usual DejaVu Sans Mono paths
  exists; name one in a config" and then calls `std::process::exit(1)`: a
  fatal condition at `warn`.
- At 101 it logs `warn` "baud on Wayland: type-checked against Linux and not
  yet run on a compositor" on **every** launch.

### 1.3 What a Dock launch does with them

The fd probe (M, [[source: ai-obs-fdprobe-run]]; the bundle and program are
[[source: ai-obs-fdprobe-info-plist]] and [[source: ai-obs-fdprobe-c]]) runs a
bundle whose executable reports where fd 0, 1 and 2 point. Each run had a
positive control, and both controls fired
([[source: ai-obs-fdprobe-stderr-direct]], [[source: ai-obs-fdprobe-stderr-open]]):

| Arm | fd 2 | Parent |
|---|---|---|
| `./fdprobe 2>err.txt` (control) | `err.txt`, marker landed | the shell |
| `open -W --stderr err2.txt FdProbe.app` (control) | `err2.txt`, marker landed | pid 1 |
| `open -W FdProbe.app` | `/dev/null`, and fd 0 and 1 too | pid 1 |
| `env -i HOME=$HOME open -W FdProbe.app` | `/dev/null`, and fd 0 and 1 too | pid 1 |

The three arms under pid 1, the `--stderr` control among them, ran as
`XPC_SERVICE_NAME=application.dev.baud.fdprobe.…`: a launchd job. The stderr
markers of the two `/dev/null` arms appear in no unified-log query, while the
same probes' os_log default lines are listed as the control (M,
[[source: ai-obs-stderr-marker-requery]], read-only, 12:57). The first
session's own marker queries are in the same file, as log(1) recorded them.

So **every line in §1.2, at every level, is discarded for a launched baud**,
and so is a panic message (§1.1). `open` passed the shell's environment
through, as open(1) says it does (§2.1), so for the environment `open` is not a
Dock launch. A Dock click itself is UNVERIFIED.

### 1.4 The premise, corrected

- **The refusal line is `warn` already**, at app.rs:3978 and linux.rs:368.
  From a terminal it shows at the default level. From the Dock nothing shows
  at any level, so the level is not what loses it.
- **The `info` line is the budget**, `describe_images`. host.rs:45-46 says so:
  "One startup line naming the resolved image budget, at `info` -- so it shows
  only under `BAUD_LOG=info` or below." It is the number a reader checks a
  changed `[images]` against. Note AG §2.1 found that "the budget line went to
  stderr, which a Dock launch discards".
- **`placements_refused` is never said, and that is already on record.** It is
  counted at `baud-domain/src/image.rs:623, 629, 634` (C). Its readers are
  `baud-bench/benches/ingest2.rs:410` and the tests in `baud-core/tests/` and
  `baud-domain/tests/`; nothing in baud-app reads it.
  - the record, "A refused Sixel says which limit it hit", lists under
    "What stays open": "**`placements_refused` is not said.**" It also lists
    "**A Dock launch still sees nothing.**", which is this note's premise.
  - The store's contract is "refuse-and-count, and the counters are public and
    never silent" (image.rs:415-416).
  - An image with no pixels is counted and deliberately not said (host.rs:68-70:
    "no limit refused it and none raised would draw it").

## 2. The texts

### 2.1 How launchd starts an app, and what its stdio is

[[source: macos-man-launchd-plist]], EXPECTATIONS: "A launchd daemon or agent
should not perform the following as part of its initialization, as launchd will
always implicitly perform them on behalf of the process. • Redirect stdio(3) to
/dev/null." The text names daemons and agents and says nothing of apps. That a
LaunchServices app gets the same treatment is the probe's finding (§1.3, M),
not the page's.

[[source: macos-man-open]]: "Opened applications inherit environment variables
just as if you had launched the application directly through its full path."
And: "--stderr PATH Launches the application with stderr connected to PATH."

[[source: macos-man-launchctl]] lists `setenv` under **LEGACY SUBCOMMANDS**:
"Legacy subcommands select the target domain based on whether they are executed
as root or not. When executed as root, they target the system domain." Then:
"setenv key value Specify an environment variable to be set on all future
processes launched by launchd in the caller's context." So it is documented,
but as a legacy subcommand.

[[source: apple-infoplist-lsenvironment]] (the Information Property List Key
Reference, an archive page marked "Updated: 2018-06-04"): "LSEnvironment
(Dictionary - macOS) defines environment variables to be set before launching
this app. … These environment variables are set only for apps launched through
Launch Services. If you run your executable directly from the command line,
these environment variables are not set."

### 2.2 The unified log: what Apple says to use

[[source: apple-logging]]: "The unified logging system provides a comprehensive
and performant API to capture telemetry across all levels of the system. This
system centralizes the storage of log data in memory and on disk, rather than
writing that data to a text-based log file. You view log messages using the
Console app, `log` command-line tool, or Xcode debug console." And: "This system
supersedes the Apple System Logger (ASL) and Syslog APIs." Among its uses:
"When you are unable to attach a debugger to the app, such as when you’re
diagnosing problems on a user’s machine."

[[source: apple-generating-log-messages]], on the subsystem: "Use reverse-DNS
notation, such as `com.example.myapp`, for each subsystem string." baud's
bundle identifier is `dev.baud.terminal` (`packaging/macos/Info.plist`, C). On
levels, verbatim:

| Log level | Persisted to disk | Notes |
|---|---|---|
| Debug | No | Captures verbose information during development that is useful only for debugging your code. |
| Info | Only when collected with the log tool | Captures information that is helpful, but not essential, to troubleshoot problems. |
| Notice (Default) | Yes, up to a storage limit | Captures information that is essential for troubleshooting problems. For example, capture information that might result in a failure. |
| Error | Yes, up to a storage limit | Captures errors seen during the execution of your code. If an activity object exists, the system captures information for the related process chain. |
| Fault | Yes, up to a storage limit | Captures information about faults and bugs in your code. If an activity object exists, the system captures information for the related process chain. |

The page goes on: "Normally, the system stores debug and info messages only in
memory, but you can write info messages to disk using the `log` command-line
tool. … When that data store exceeds a predefined size, the system purges old
messages to make room for new ones." On cost: "Debug logs have very low overhead
because the system stores them only in memory. Faults and other severe messages
incur more overhead because the system often captures additional information
and writes all of that information to disk."

Two Apple texts word the default type differently:
- [[source: macos-sdk-man-os-log]]: `os_log` is 'a "default" type of log
  message that is always captured in memory and on disk'.
- [[source: macos-sdk-os-log-h]], lines 220-221: os_log messages "cannot be
  disabled and therefore always captured either to memory or disk".

Of `os_log_info`, os_log(3) says the entries "are normally appended to a global
memory buffer, and the most recent ones are written out at collection time".

**Measured: what the store keeps.**
- [[source: ai-obs-review-info-debug-absent]] (saved by this note's reviewer)
  was read at 11:59 with `--info --debug` over 11:00-11:40. It lists every
  Default, Error and Fault line of this session's probes, which is the
  positive control, and no Info or Debug line.
- [[source: ai-obs-lifeprobe]] holds the controlled run
  ([[source: ai-obs-lifeprobe-c]]): one Default (the control), one Info and
  one Debug line at 12:05:34. They were read immediately and about 10, 20 and
  30 minutes later, with nothing of this session logging in bulk in between.
  - Immediately (12:05:34), the Default and Info lines were listed and the
    Debug line was not. The Debug line was never emitted: Debug is disabled
    here (fdprobe printed `debug_enabled=0`, [[source: ai-obs-fdprobe-run]];
    the disabled call costs under a tick, §4).
  - At 12:15:43, 12:25:44 and 12:35:45 the Info line was gone, and so was
    capprobe2's `os_log_info` line from 12:05:02. Both Default control lines
    and capprobe2's Error and Fault lines were listed at every reading.
  - So on this Mac, under its own background load, an info line was gone by
    about ten minutes. The first reading without it came 10 min 09 s after it
    was logged, and 10 min 41 s after capprobe2's.
- This establishes no lifetime for an info line on another machine or under
  other load. It agrees with Apple's table and with os_log(3).
- The first draft's 11:20 reading is not used. Its query was not saved, and it
  came 22 s after this session's own burst of 6000 persisted lines
  ([[source: ai-obs-review-costprobe-lines]]).

**Privacy.** [[source: macos-sdk-man-os-log]]: "The unified logging system
considers dynamic strings and complex dynamic objects to be private, and does
not collect them automatically. To ensure the privacy of users, it is
recommended that log messages consist strictly of static strings and numbers,
which are collected automatically by the system." It defines `public` as
"Declares that an argument should always be visible in logs, regardless of
privacy settings. Use this for data that would not compromise user privacy.
For example, %{public}s." [[source: apple-generating-log-messages]]: "By
default, the system doesn’t redact integer, floating-point and Boolean values,
but it does redact the contents of dynamic strings and complex dynamic
objects." [[source: apple-oslogprivacy]]: "Because people have access to log
messages that your app generates, use the privacy options to hide potentially
sensitive information."

Measured (M). The probes are [[source: ai-obs-privprobe-c]] and
[[source: ai-obs-nslogprobe-m]]. The outputs are [[source: ai-obs-log-show-probes]]
(its query in [[source: ai-obs-log-invocations]]) and
[[source: ai-obs-review-info-debug-absent]]. NSLog's unified-log line is in
[[source: ai-obs-stderr-marker-requery]] and its stderr line in
[[source: ai-obs-nslogprobe-stderr]].

| Probe | Result |
|---|---|
| `%s` of a heap string | `<private>` |
| `%{public}s` of a heap string | shown |
| `%s` of a string literal | shown (why: UNVERIFIED) |
| `NSLog`'s `%@` | `<private>` in the unified log, in full on stderr |

baud's lines are all built at run time. Through `%s` every one would read
`<private>`, so anything the unified log is to show has to be `%{public}s`,
which is a choice per value (§7, question 5).

**Reading it.**
- [[source: apple-viewing-log-messages]]: "you can’t read and parse the log
  files directly; you need to use tools".
- [[source: macos-man-log]]: `show` "The output contains only default level
  messages unless --info and/or --debug are specified". `subsystem` "Only works
  with log messages generated with os_log(3) APIs". Its own example is
  `log show --predicate 'subsystem == "com.example.my_subsystem"'`.
- [[source: apple-console-log-messages]]: "If you’re not logged in as an
  administrator, you need to enter an administrator name and password to view
  log messages". The live view starts when you "Click Start in the toolbar".
- A `log show` without sudo exited 0 here for a user in `admin` (M,
  [[source: ai-obs-admin-and-shim]]). A user outside `admin` was not tried
  (UNVERIFIED).
- Keeping more than the defaults is a debugging tool. The page is
  [[source: apple-customizing-logging]], titled "Customizing Logging Behavior
  While Debugging": "Logging behavior is normally governed by the system.
  However, while debugging in macOS, you can enable different logging levels
  for a subsystem using the `log` command-line tool’s `config` argument while
  logged in as root." It adds a profile under
  `/Library/Preferences/Logging/Subsystems/`.

**Length.** os/log.h gives "a physical cap of 1024 bytes per log line for
dynamic content" for `os_log` (227-230). It gives "256 bytes per entry" for
`os_log_info`, `os_log_debug`, `os_log_error` and `os_log_fault` (259-262,
289-292, 329-332, 387-390). Those sentences cap what "can be written to the
persistence store", and add: "Live streams will continue to show the full
content." Measured (M):

| Call | Logged | Kept |
|---|---|---|
| `os_log_with_type` at Default ([[source: ai-obs-longline-reread]], re-read 12:09) | 1999 bytes of `%{public}s` | 1015 bytes, then `<…>` |
| `os_log_with_type` at Error, Default and Fault, i.e. `_os_log_impl`, not the documented error and fault macros ([[source: ai-obs-capprobe-c]]; [[source: ai-obs-capprobe-reread]], re-read 12:04, logged 11:26) | 600 bytes each | all 600 |
| the documented macros `os_log_error`, `os_log_fault` and `os_log_info` ([[source: ai-obs-capprobe2-c]]; [[source: ai-obs-capprobe2]], read at once) | 600 bytes each | all 600 |
| the same Error and Fault lines, re-read 12:36 ([[source: ai-obs-lifeprobe]]); the Info line was gone by 12:15 | -- | all 600 (622 bytes each) |

So the header's 1024 for the default type held, near enough. For Error and
Fault, 600 bytes held in the store half an hour on, past the header's 256. For
Info and Debug the probe cannot reach the cap:
- The Info line was listed only once, at once, which a live read may serve, and
  it was gone by 12:15:43, so it was never shown persisted.
- Debug was never emitted (disabled here).
[[source: apple-os-log-with-type]] says only that "the logging system may
truncate lengthy log messages when it stores them".

A `describe_refusals` line with all three causes runs past 256 bytes (C, by its
three format strings). Whether a cap matters to the level chosen is open.

### 2.3 The C surface, for a caller that is neither C nor Swift

[[source: macos-sdk-os-log-h]], verbatim with line numbers:

```c
104 OS_ENUM(os_log_type, uint8_t,
105     OS_LOG_TYPE_DEFAULT = 0x00,
106     OS_LOG_TYPE_INFO    = 0x01,
107     OS_LOG_TYPE_DEBUG   = 0x02,
108     OS_LOG_TYPE_ERROR   = 0x10,
109     OS_LOG_TYPE_FAULT   = 0x11,
110 ) LOG_SWIFT_NAME(OSLogType);
...
134  * logging runtime maintains a global collection of all os_log_t objects, one
135  * per subsystem/category pair.  The os_log_t for a given subsystem/category is
136  * created upon the first call to os_log_create and any subsequent calls return
137  * the same object.  These objects are never deallocated, so dynamic creation
138  * (e.g. on a per-operation basis) is generally inappropriate.
...
142 API_AVAILABLE(macos(10.12), ios(10.0), watchos(3.0), tvos(10.0))
143 OS_EXPORT OS_NOTHROW OS_WARN_RESULT OS_OBJECT_RETURNS_RETAINED OS_NONNULL_ALL
144 os_log_t
145 os_log_create(const char *subsystem, const char *category)
...
202 #define os_log_with_type(log, type, format, ...) __extension__({ \
203     os_log_t _log_tmp = (log); \
204     os_log_type_t _type_tmp = (type); \
205     if (os_log_type_enabled(_log_tmp, _type_tmp)) { \
206         OS_LOG_CALL_WITH_FORMAT(_os_log_impl, \
207                 (&__dso_handle, _log_tmp, _type_tmp), format, ##__VA_ARGS__); \
208     } \
209 })
...
443 API_AVAILABLE(macos(10.12), ios(10.0), watchos(3.0), tvos(10.0))
444 OS_EXPORT OS_NOTHROW OS_WARN_RESULT OS_PURE
445 bool
446 os_log_type_enabled(os_log_t oslog, os_log_type_t type)
...
449 /*!
450  * @function _os_log_impl
451  *
452  * @abstract
453  * Internal function that takes compiler generated encoding and captures the necessary content.
454  */
455 API_AVAILABLE(macos(10.12), ios(10.0), watchos(3.0), tvos(10.0))
456 OS_EXPORT OS_NOTHROW OS_NOT_TAIL_CALLED
457 void
458 _os_log_impl(void *dso, os_log_t log, os_log_type_t type,
459         const char *format, uint8_t *buf, uint32_t size);
```

The format "must be a constant string, not dynamically generated" (199-200).
[[source: macos-sdk-os-trace-base-h]] shows what the macro expands to:

```c
115 #ifndef OS_LOG_STRING_SECTION
116 #define OS_LOG_STRING_SECTION "__TEXT,__oslogstring,cstring_literals"
117 #endif
...
128 #define OS_LOG_CALL_WITH_FORMAT(fun, fun_args, fmt, ...) __extension__({ \
129         OS_LOG_PRAGMA_PUSH OS_LOG_STRING(LOG, _os_fmt_str, fmt); \
130         __attribute__((stack_protector_ignore)) \
131         uint8_t _Alignas(16) OS_LOG_UNINITIALIZED _os_fmt_buf[__builtin_os_log_format_buffer_size(fmt, ##__VA_ARGS__)]; \
132         fun(OS_LOG_REMOVE_PARENS fun_args, _os_fmt_str, \
133                 (uint8_t *)__builtin_os_log_format(_os_fmt_buf, fmt, ##__VA_ARGS__), \
134                 (uint32_t)sizeof(_os_fmt_buf)) OS_LOG_PRAGMA_POP; \
135 })
...
163 extern struct mach_header __dso_handle;
```

- [[source: macos-sdk-os-apinotes]] marks `_os_log_impl` `SwiftPrivate: true`.
- [[source: macos-sdk-libsystem-trace-tbd]] lists `_os_log_create`,
  `_os_log_type_enabled`, `__os_log_impl` and `__os_log_default` among its
  exports. Its lines 7-10 name `parent-umbrella:` … `umbrella: System`, so the
  symbols come with libSystem and linking needs nothing new.

What follows for baud, which declares foreign functions in
`baud-app/src/sys.rs` and holds their layouts to the headers with a C probe
compiled at test time (C):

- **Documented and declarable.** `os_log_create` and `os_log_type_enabled` are
  functions. `os_log_type_t` is a `uint8_t` with the five values above, and the
  existing probe test can hold them to the header.
- **Not callable as documented.** `os_log_with_type` is a macro, and so is
  `os_log`: [[source: apple-os-log]] documents it as
  `#define os_log(log, format, ...)`.
  - The macro's format string goes in `__TEXT,__oslogstring`. Rust can place a
    static in a named section: [[source: rust-reference-link-section]], "The
    *`link_section` attribute* specifies the section of the object file that a
    [function] or [static]'s content will be placed into", and the attribute
    is unsafe.
  - What Rust cannot reproduce is the buffer `__builtin_os_log_format` writes.
    The only function the macro calls is described as an "Internal function
    that takes compiler generated encoding", and no Apple text read here
    documents that encoding.
  - So a buffer built by hand in Rust would rest on the compiler's undocumented
    layout: a departure from the documented surface.
- **A C shim stays on the documented surface.** It is one function that calls
  `os_log_with_type(log, type, "%{public}s", message)`, which is ghostty's
  `zig_macos.c` exactly (§3).
  - baud's precedent for compiling C is `baud-sfte-difftest/build.rs`:
    "**A raw `cc` invocation, not the `cc` crate**, because there is no
    `[build-dependencies]` anywhere in this workspace" (C). That crate is a
    test oracle, though. baud-app's `build.rs` compiles nothing today, so a
    shim would be the first C object in the shipping binary.
  - Measured (M; [[source: ai-obs-shim-c]], [[source: ai-obs-shimmain-c]];
    [[source: ai-obs-admin-and-shim]], [[source: ai-obs-shim-run]],
    [[source: ai-obs-shim-reread]]): the four-line shim compiles under the devenv
    shell's own `cc` (nix clang 21.1.8 with nix apple-sdk 14.4, not Xcode's
    27.0 SDK). It puts `%{public}s` in `(__TEXT,__oslogstring)` and logs a
    heap string in full at Default and at Error. The same SDK reports
    `sizeof(os_log_type_t)` = 1.
- **`syslog(3)` needs no C.** Measured (M; [[source: ai-obs-syslogprobe-c]],
  [[source: ai-obs-syslogprobe2-c]]; [[source: ai-obs-syslogprobe1]], its query
  in [[source: ai-obs-log-invocations]]; [[source: ai-obs-syslogprobe1-reread]];
  [[source: ai-obs-syslogprobe2]]):
  - Its lines land in the unified log with no stderr output, and a heap string
    through plain `%s` is shown in full.
  - `LOG_CRIT`, `LOG_ALERT`, `LOG_ERR`, `LOG_WARNING` and `LOG_NOTICE` all
    land as Default, `LOG_INFO` lands as Info, and `LOG_DEBUG` is not listed.
    `LOG_EMERG` was not sent. [[source: macos-sdk-man-syslog]] (roff lines
    127-128) defines it as "A panic condition. This is normally broadcast to
    all users.", so it was not tried on the owner's machine.
  - The legacy `_syslog` symbol behaves the same at `LOG_WARNING`, `LOG_ERR`
    and `LOG_INFO`. [[source: macos-sdk-sys-syslog-h]] line 227 aliases
    `syslog` to `syslog$DARWIN_EXTSN` from 10.13, so a plain Rust `extern`
    binds the legacy symbol unless it names the other.
  - os_log's `%{private}s` and `%{public}s` in syslog's format both came out as
    `<decode: missing data>`: the value was lost. Clang also warns that the
    annotation is "outside of os_log()/os_trace()" (M,
    [[source: ai-obs-syslogprobe2-syntax]]). So syslog offers no
    per-value privacy. A value is shown whole or left out of the line.
  - Apple's own words count against it: unified logging "supersedes … Syslog
    APIs" (§2.2), and `subsystem` "Only works with log messages generated with
    os_log(3) APIs" (log(1)). A reader filters by `process == "baud"` instead.
  - [[source: macos-sdk-man-syslog]], dated 1993, says nothing about the
    unified log.

### 2.4 Log files on macOS

- [[source: apple-fspg-library-dirs]] is the File System Programming Guide, an
  archive page marked "Updated: 2018-04-09". Its Table A-1, "Subdirectories of
  the Library directory", says: "Logs: Contains log files for the console and
  specific system services. Users can also view these logs using the Console
  app."
- [[source: apple-console-reports]] (macOS 27): "Log Reports: System and user
  reports with information about events that occur when the system or specific
  apps are processing. Log report names have an extension such as .log, ._log,
  or .its." … "User reports are from apps used by the current user."
- [[source: apple-console-locate-log-file]] finds one through File › Reveal in
  Finder.
- Foundation's `NSSearchPathDirectory` has no logs member: a search of
  [[source: macos-sdk-nspathutilities-h]] found none.
- Whether Console's Log Reports lists a file in a subdirectory,
  `~/Library/Logs/baud/`, is UNVERIFIED: checking needs the GUI.

So a 2018 archive text names `~/Library/Logs` for log files and Console as
their reader. Apple's current guidance for an app's own messages is the unified
log, "rather than writing that data to a text-based log file" (§2.2).

### 2.5 Linux

[[source: systemd-journald-service]]: journald collects "Simple system log
messages, via the libc syslog(3) call", "Structured system log messages via the
native Journal API", and "Standard output and standard error of service units".
On access: "Journal files are, by default, owned and readable by the
systemd-journal system group but are not writable." And: "By default, each
user, with a UID outside the range of system users, dynamic service users, and
the nobody user, will get their own set of journal files in /var/log/journal/.
… Instead, file system ACLs are used to ensure the user gets read access only."

**The native protocol.** [[source: systemd-journal-native-protocol]]:
- It "is spoken on the `/run/systemd/journal/socket` `AF_UNIX`/`SOCK_DGRAM`
  socket on which `systemd-journald.service` listens. Each datagram sent to
  this socket encapsulates one journal entry".
- "The most important key/value pair to send is `MESSAGE=` … Other relevant
  keys a client should send in most cases are `PRIORITY=`, `CODE_FILE=`,
  `CODE_LINE=`, `CODE_FUNC=`, `ERRNO=`."
- On cost: "A client probably should bump up the `SO_SNDBUF` socket option of
  its `AF_UNIX` socket towards `journald` in order to delay blocking I/O as
  much as possible."
- On stderr: "If a program wants to detect automatically whether its STDERR is
  connected to the Journal's stream transport, look for the `$JOURNAL_STREAM`
  environment variable."
- Its line 13 lists "STDOUT/STDERR of programs via `StandardOutput=journal` +
  `StandardError=journal` in service files (both of which are default
  settings)".
- The document exists for clients "without linking to `libsystemd`".
  baud-wayland's `sys` already declares `sendmsg`
  (`baud-wayland/src/sys.rs:147`, C).
- [[source: systemd-sd-journal-print]]: "If systemd-journald(8) is not running
  (the socket is not present), those functions do nothing, and also return 0."

**For a service unit,** [[source: systemd-exec]], on StandardOutput=: "If
StandardInput= is set to one of tty, tty-force, tty-fail, socket, or fd:name,
this setting defaults to inherit. In other cases, this setting defaults to the
value set with DefaultStandardOutput= in systemd-system.conf(5), which defaults
to journal." On StandardError=: "if set to inherit the file descriptor used for
standard output is duplicated for standard error", and it "defaults to the
value set with DefaultStandardError= in systemd-system.conf(5), which defaults
to inherit". So by default a service's stderr reaches the journal too.

**Desktops.** [[source: systemd-desktop-environments]], "a work-in-progress",
says "desktop environments should adhere" to a scheme in which "Application
units should follow the scheme `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service`
or `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope`". It adds: "Using `.service`
units instead of `.scope` units … is encouraged".

**Practice, one desktop.** [[source: gnome-shell-app-launch]]'s
`shell_app_launch` hands a launched app `sd_journal_stream_fd
(shell_app_get_id (app), LOG_INFO, FALSE)` as both stdout and stderr
(shell-app.c:1432-1444). [[source: systemd-sd-journal-stream-fd]] gives the
arguments' meaning:
- The identifier "will be written to the journal as SYSLOG_IDENTIFIER= field".
- "The second argument shall be the default priority level for all messages."
- "The third argument is a boolean: if true kernel-style log level prefixes
  (such as SD_WARNING) are interpreted".
- "O_NONBLOCK is turned off initially".

So on GNOME, baud's stderr lands in the user's journal under its app id, every
line at priority info, through a descriptor whose writes can wait. Other
desktops and launchers are UNVERIFIED.

[[source: xdg-basedir-0-8]]: "$XDG_STATE_HOME defines the base directory
relative to which user-specific state files should be stored. If
$XDG_STATE_HOME is either not set or empty, a default equal to $HOME/.local/state
should be used. The $XDG_STATE_HOME contains state data that should persist
between (application) restarts … It may contain: actions history (logs, history,
recently used files, …)". And: "If, when attempting to write a file, the
destination directory is non-existent an attempt should be made to create it
with permission 0700."

## 3. Prior art, at source

| Terminal (pinned) | macOS | Elsewhere | Default level | Platform text |
|---|---|---|---|---|
| ghostty v1.3.1 | unified log, subsystem = bundle id, category = scope, `%{public}s` through a C shim; the app's Zig core logs no stderr (inference, below) | stderr | info (release builds) | the shim stays on the documented surface; it looks up its log object on every call, where the header calls per-operation creation "generally inappropriate", and maps err to Fault |
| kitty v0.49.2 | unified log, `os_log_error(OS_LOG_DEFAULT, "%{public}s", …)`, when LaunchServices launched it; else stderr | stderr | one level, `log_error` | follows it, with no subsystem |
| alacritty v0.17.0 | file `$TMPDIR/Alacritty-<pid>.log`, made on the first record, plus stdout; warnings and errors also shown in the window | same | warn | neither the unified log nor `~/Library/Logs` |
| wezterm 20240203 | file `RUNTIME_DIR/<exe>-log-<pid>.txt`, `RUNTIME_DIR` = `~/.local/share/wezterm` on a Mac; plus stderr and an in-memory ring | Linux: `$XDG_RUNTIME_DIR/wezterm/`, else `~/.local/share/wezterm` | info | not the platform's on macOS; Linux uses the runtime dir, not `$XDG_STATE_HOME` |
| foot 1.28.0 | -- | stderr always; `syslog(3)` in server mode | warning | Linux: syslog(3) is a source journald collects |
| iTerm2 master 964a75d | `ELog` and `ILog`: a rotating file written on every call, plus `NSLog` for `ELog`; opt-in debug log to `/tmp/debuglog.txt` and, optionally, a unified-log line without the message | -- | -- | `NSLog` reaches the unified log (M, §2.2); no platform text read here covers `NSLog` |

**ghostty.**
- [[source: ghostty-os-log-zig]] declares `extern "c" fn
  zig_os_log_with_type(*Log, LogType, [*c]const u8) void` and formats the
  message in Zig first.
- [[source: ghostty-zig-macos-c]] is the whole shim: "A wrapper so we can use
  the os_log_with_type macro" … `os_log_with_type(log, type, "%{public}s",
  message);`.
- [[source: ghostty-main-logfn]]'s `logFn` maps `.warn => .err` and
  `.err => .fault`. Apple's Fault is "about faults and bugs in your code"
  (§2.2), so mapping every error to it departs from that text.
- Its comment says to read the log with `sudo log stream --level debug
  --predicate 'subsystem=="com.mitchellh.ghostty"'`.
- It calls `os_log_create` and `os_release` on every call ("This is slow to
  do on every operation but we shouldn't be logging too much"). Per log.h
  134-137, later calls return the same object, and `os_log_create` is
  `OS_OBJECT_RETURNS_RETAINED` (143), so the release balances it. Lines 137-138
  still call per-operation creation "generally inappropriate".
- Its "macOS logging is thread safe" is ghostty's claim, not Apple's: thread
  safety is UNVERIFIED in the texts read here.
- [[source: ghostty-global-logging]]: `stderr: bool = build_config.app_runtime
  != .none` under the comment "For lib mode we always disable stderr logging by
  default. Otherwise it's enabled by default.", and `macos: bool =
  builtin.os.tag.isDarwin()`, overridden by `GHOSTTY_LOG`.
- That the macOS app runs the core in lib mode, and so logs no stderr, is an
  inference. AppDelegate.swift:295 speaks of reporting a value "to libghostty"
  ([[source: ghostty-appdelegate-logger]]), and main_ghostty.zig:74-80 calls
  the `.none` executable "the Ghostty helper CLI that accompanies the graphical
  Ghostty app".
- Its Swift side uses `Logger(subsystem: Bundle.main.bundleIdentifier!, …)`
  (AppDelegate.swift:15-18).

**kitty.**
- [[source: kitty-setup-lsenvironment]] sets
  `LSEnvironment={'KITTY_LAUNCHED_BY_LAUNCH_SERVICES': '1'}`.
- [[source: kitty-launcher-lbls]] reads that variable and unsets it.
- [[source: kitty-main-os-log]] calls `set_use_os_log(True)` under `if is_macos
  and launched_by_launch_services and not called_from_panel:`.
- [[source: kitty-logging-c]] then writes `os_log_error(OS_LOG_DEFAULT,
  "%{public}s", sanbuf)` and skips its own timestamp ("Apple's os_log already
  records timestamps").
- Before that it rewrites the bytes: `C0_EXCEPT_NL_SPACE_TAB_DEL` becomes
  `0x2400 + byte` (Control Pictures, so newline and tab are kept), and DEL
  `0x7f` becomes U+2421. The macro's body was not downloaded. This is worth
  copying for any line that carries bytes a child wrote.

**alacritty.**
- [[source: alacritty-logging-rs]]: `OnDemandLogFile` at
  `env::temp_dir()/Alacritty-<pid>.log`, opened
  `append(true).create_new(true)` on the first record that passes the level.
- Each record is written under a `Mutex` on the calling thread.
- Every Error and Warn record also goes to the window's message bar as
  "See log at <path> ($ALACRITTY_LOG)".
- It sets `ALACRITTY_LOG` in its own environment (logging.rs:204). That the
  child inherits it is inferred; the spawn code was not downloaded.
- [[source: alacritty-cli-loglevel]]: the default is `LevelFilter::Warn`.

**wezterm.**
- [[source: wezterm-ringlog]] writes stderr, an in-memory ring the GUI can
  show, and `RUNTIME_DIR/<exe>-log-<pid>.txt`.
  - `<exe>` is the running executable's file name, so a GUI process named
    `wezterm-gui` writes `wezterm-gui-log-<pid>.txt`. No file read here names
    the macOS executable.
  - `RUNTIME_DIR` is `compute_runtime_dir()` ([[source: wezterm-config-lib]]).
- It deletes `-log-` files over a week old when the GUI starts, and defaults to
  `LevelFilter::Info` unless `WEZTERM_LOG` is set.
- [[source: wezterm-runtime-dir]] uses `dirs_next::runtime_dir()/wezterm`,
  else `HOME/.local/share/wezterm`. [[source: dirs-next-runtime-dir]] answers
  `None` on macOS, so a Mac uses `~/.local/share/wezterm`. On Linux the
  crate's `src/lin.rs:9` passes `XDG_RUNTIME_DIR` through
  `dirs_sys_next::is_absolute_path`, whose body was not read. Without the
  variable, Linux gets the same fallback.

**foot.**
- [[source: foot-main-log-init]]: `log_init(log_colorize, as_server &&
  log_syslog, …)`, with the default `LOG_CLASS_WARNING`.
- [[source: foot-log-c]] always writes stderr, and calls `syslog(level, "%s: %s",
  module, msg)` only when `do_syslog` is set.

**iTerm2.**
- [[source: iterm2-debuglogging-h]]: `ELog` is `DLog` + `LogForNextCrash` +
  `NSLog`, and `ILog` is `DLog` + `LogForNextCrash`. Both carry the comment
  "no private info. Low-volume. Logged to crash reports."
- [[source: iterm2-debuglogging-m]]'s `LogForNextCrash` (461-506) runs on
  every such call, not opt-in:
  - It writes each line to `log.<n>.txt` in the directory iTerm2's
    `applicationSupportDirectory` returns. That method's definition was not
    downloaded.
  - `n` rotates over 0-2 every 100 lines.
  - Each write is followed by `synchronizeFile` on the calling thread.
- `DebugLogImpl` (402-459) runs only while debug logging is on:
  - It always appends the entry to a buffer, `gDebugLogStr` (448), which
    `FlushDebugLog` (198-239) writes to `/tmp/debuglog.txt`.
  - With `logToSyslog` it also sends `os_log_with_type(OS_LOG_DEFAULT,
    OS_LOG_TYPE_DEBUG, "iTerm2DebugLog: %{public}s", message.UTF8String)`. That
    `message` holds only the timestamp, file, line and function (416-422); the
    entry's text is never passed.
- `NSLog`'s dynamic `%@` showed as `<private>` in the unified log (M, §2.2).
  So an `ELog` with object arguments would show there redacted. That is
  inferred from the NSLog probe, not from iTerm2.

## 4. Cost: scheduled, never waited

The project's rule is that nothing on the main thread blocks or waits. The
probe logged one 142-byte line (143 with the newline the write rows add) 1000
times back to back per row, in three runs. Measured (M,
[[source: ai-obs-costprobe-rerun]], 12:04, from [[source: ai-obs-costprobe-c]];
a cell lists the three runs in order where they differ). It measures
`os_log_with_type` called from C, the shim's route, and `write(2)`. `syslog(3)`,
a buffer built by hand and journald's `sendmsg` were not measured. The file was
written on the file system `~/Library/Logs` is on (M,
[[source: ai-obs-stat-volume]]). The timebase is 125/3, so one tick is
41.67 ns, "0 ns" means under one tick and "42 ns" is one tick.

| Call | Median | p99 | Max |
|---|---|---|---|
| `os_log` Default, `%{public}s` | 125 ns | 35.3, 30.0, 36.1 us | 114.7, 77.8, 68.3 us |
| `os_log` Error, `%{public}s` | 125 ns | 34.3, 4.3, 3.4 us | 63.5, 62.3, 46.7 us |
| `os_log` Debug, disabled | 0 ns | 42 ns | 125, 83, 42 ns |
| `write(2)`, `O_APPEND` regular file | 1375, 1333, 1250 ns | 3.75, 3.54, 3.54 us | 19.9, 6.9, 13.5 us |
| `write(2)`, `/dev/null` | 667, 667, 625 ns | 833 ns | 7.3, 4.0, 1.3 us |

**What the table shows.**
- `os_log` is 10 to 11 times cheaper than a file write at the median.
- At p99 the Default type is 8.0 to 10.2 times dearer (over every pairing of
  runs), which reverses the median ranking. The Error type's p99 ranged from
  3.4 to 34.3 us.
- Error's slowest call was 47 to 64 us, and Default's 68 to 115 us. The file's
  was 7 to 20 us.
- An earlier session, whose output was transcribed and not saved, showed larger
  maxima: up to about 1.1 ms for `os_log` and 0.32 ms for the file write
  ([[source: ai-obs-index]] §6, marked TRANSCRIBED). It is not tagged M.
- A burst of 1000 is not baud's pattern. The refusal line comes at most once a
  pump, and nothing logs per byte (C).
- A tail is not shown to be a wait. The `/dev/null` arm is the control for
  what the loop and the scheduler add to a trivial system call, and its
  slowest call was 1.3 to 7.3 us. The `os_log` maxima stand well above that;
  the file's (7 to 20 us) partly overlap it. What the tails are is UNVERIFIED,
  and Apple's texts read here do not say whether `os_log` can wait.

**Each sink against the rule.**
- **A file** has to be written by a thread of its own, which owns the
  descriptor and is fed lines over a channel, the way the pty reader owns the
  master. Whether `O_NONBLOCK` changes anything for a regular file on Darwin is
  UNVERIFIED.
- **`os_log`** would be called where the line is made, as now. The pty reader
  thread logs too (pty.rs:911), so it matters whether calls from two threads
  are safe, and the texts read here do not say (UNVERIFIED). A writer thread
  would take the tail off the main thread for `os_log` as well. The cost is
  that the timestamp becomes the writer thread's, not the event's.
- **journald's socket** can block. The protocol's own advice is to raise
  `SO_SNDBUF` "to delay blocking I/O". Sending with `MSG_DONTWAIT` would drop a
  line rather than wait; that is a design choice, UNVERIFIED against journald.
- **stderr as it is** can block too, wherever it is a pipe or a socket. GNOME's
  journal stream is one: "O_NONBLOCK is turned off initially" (§2.5). And
  `eprintln!` panics on a write error (§1.1).
- **A disabled level** costs under a tick at the median and one tick at p99
  through `os_log_type_enabled` (M). So under the unified log the level gate
  could be the system's own, letting the OS decide what is kept.

## 5. The options

**(a) The unified log on macOS, and journald or stderr on Linux.**
- *What changes:* `log.rs` sends each line to `os_log` under subsystem
  `dev.baud.terminal`. A category could name the area (`images`, `config`,
  `render`, `link`, `pty`); its cost is unmeasured. On Linux the line goes
  to the journal's native socket when present, and to stderr otherwise. The
  macOS route is one of these (§7, question 2):
  - a C shim;
  - `syslog(3)`;
  - a buffer built in Rust for `_os_log_impl`.
- *Conformance:*
  - macOS conforms through the shim. `syslog` is the API Apple says is
    superseded. A hand-built buffer departs from the documented surface.
  - On Linux the native protocol is journald's documented interface. stderr
    reaches journald by its own texts only for service units, and GNOME's
    routing of it is practice.
- *Cost:* for the shim, the medians and tail in §4, which measured
  `os_log_with_type` from C. `syslog(3)`, a hand-built buffer and journald's
  `sendmsg` were not measured. The thread question is open, and there is
  nothing new to link (libSystem).
- *Privacy:* per value, `%{public}s` or `%{private}s` (§7, question 5).
  - Admins can read the store, and Console asks anyone else for an admin
    password (§2.2).
  - Info lines are persisted "Only when collected with the log tool"; Default
    and Error lines "Yes, up to a storage limit" (Apple's table, §2.2).
  - Through `syslog` a value is shown whole or left out (M, §2.3).
- *How a user reads it:* `log show --last 1h --predicate 'subsystem ==
  "dev.baud.terminal"'`, or `log stream --level info --predicate …` live, or
  Console after Start.
  - On Linux, `journalctl --user -t baud` (UNVERIFIED: the identifier is
    whatever `SYSLOG_IDENTIFIER=` baud sends; on GNOME's stderr route it is the
    app id).

**(b) A file: `~/Library/Logs/baud/` on macOS, `$XDG_STATE_HOME/baud/` on Linux.**
- *What changes:* a writer thread appends timestamped lines to a file, created
  on the first line (as alacritty does) or at launch. The file needs a bound:
  size, per-pid files, or pruning by age (wezterm prunes at a week).
- *Conformance:*
  - Linux: the XDG spec says state "may contain … logs", and asks for 0700
    directories.
  - macOS: a 2018 archive text names `~/Library/Logs` for log files and Console
    as their reader. Apple's current guidance for an app's messages is the
    unified log, so the macOS file is practice that an older text allows.
- *baud's own idiom* points at XDG paths on macOS too. Config is read from
  `$XDG_CONFIG_HOME/baud` and `~/.config/baud` (config.rs:18-20, C), not from
  `~/Library`.
- *Cost:* the writer thread (§4), and disk.
- *Privacy:* mode 0600 in a 0700 directory, readable by the user and root.
  Nothing redacts, so link URIs and paths are there in full unless left out.
- *How a user reads it:* `cat` or `tail -f` on the file. Console's Log Reports,
  if it lists `~/Library/Logs/baud/` (UNVERIFIED). baud could print the path,
  or show it in the window as alacritty does.

**(c) Both.**
- *What changes:* the unified log as the platform's record, and a file a user
  can attach to a report without `log show` or admin rights.
- *Conformance:* the unified-log half conforms (as (a)). The file half is as
  (b): XDG on Linux, practice on macOS.
- *Cost:* everything above, twice, and two privacy policies for one line.

**(d) Keep stderr and raise the level.**
- *What changes:* only what a terminal launch prints. It **fixes nothing for a
  Dock launch**: fd 2 is `/dev/null` at every level (§1.3, M). On GNOME it puts
  more lines in the journal, all at priority info.
- *Conformance:* no text read here governs an app's stderr on macOS. For a
  launchd job, launchd.plist(5) sends it to `/dev/null`. On Linux, stderr
  reaches journald by its own texts only for service units.
- *Cost:* nothing new beyond today's `eprintln!`. It can block on a pipe or on
  GNOME's stream, and it panics on a write error (§4).

**(e) Practice only: say it in the window.**
- *What changes:* alacritty shows every warning and error in a message bar with
  the log's path. That reaches a user who never opens a log. The yazi blank was
  invisible to the person looking at it, which no log fixes alone.
- *Conformance:* no platform text bears on it.
- *Cost:* a surface the renderer draws and a rule for when it shows and goes;
  not built and not measured here.

## 6. Levels

| baud | `os_log_type_t` | Apple's description (§2.2) | What it covers today |
|---|---|---|---|
| `err!` | Error (0x10) | "Captures errors seen during the execution of your code." | the compositor closed (Linux) |
| `warn!` | Default (0x00) **or** Error (0x10) | Default: "Captures information that is essential for troubleshooting problems. For example, capture information that might result in a failure." | refusals, fallbacks, config mistakes, links refused; and on Linux the fatal "no font" (linux.rs:109) |
| `info!` | Info (0x01) | "Captures information that is helpful, but not essential, to troubleshoot problems."; persisted "Only when collected with the log tool" | the budget, config path, faces, the five-second report, links opened |
| `debug!`, `verbose!` | Debug (0x02) | "Captures verbose information during development that is useful only for debugging your code." | unused today (C) |

Fault "Captures information about faults and bugs in your code", and none of
baud's lines is one. ghostty maps warn to Error and err to Fault (§3).

- **Refusal lines are `warn` already** (§1.4). Under the unified log `warn` is
  persisted either way, as Default or as Error. Console marks an Error with a
  dot: "Yellow: Errors" ([[source: apple-console-log-messages]]).
- **The budget line** is `info`, so under the unified log it is persisted only
  when collected. As Default it would be one persisted line a launch. For
  raising it: it is the number a changed `[images]` is checked against. Against
  raising it: the refusal line already names the limit it hit, with its size and
  the `[images]` key that raises it (§1.2).
- **`placements_refused`** has no line (§1.4).
- **linux.rs:109** logs at `warn` and then exits. Apple's Error text is
  "Captures errors seen during the execution of your code", and Default's is
  "Captures information that is essential for troubleshooting problems"; which
  it is, is the owner's call with question 4.

## 7. The owner's questions

1. **Where should a launched baud's lines go on macOS?**
   - **(a) The unified log.** It is governed by Apple's current Logging pages.
     Its bytes go to the system's store, in memory and on disk, read with
     `log` or Console (admin rights to view in Console). Cost: through the
     shim, a 125 ns median with a tail to tens of microseconds in a burst
     (§4); the other routes are unmeasured.
   - **(b) A file under `~/Library/Logs/baud/`.** It is governed by a 2018
     archive text that names that directory for log files. Its bytes go to a
     text file baud writes, readable by the user. Cost: a 1.3 us median write
     with a tail to 20 us (§4), on a writer thread of its own, plus a bound on
     the file.
   - **(c) Both.** Cost: both of the above.
   - (a) conforms to the current guidance. (b) is practice an older text
     allows. (c) is both, with both costs. (d), stderr at a higher level, does
     not reach a Dock launch at all.
   - **Decided: (a), the unified log** (Jev, 2026-10-09, 0.97 and 1.0 on the
     original and on a replicate with the options reordered, decision log
     lines 353-354; to be listed on the review page for the owner's veto).
     It conforms: Apple's text names the unified log as superseding ASL and
     Syslog (§2.2). A launched baud's lines go to the unified log under
     subsystem `dev.baud.terminal` (§5 (a)), and a terminal launch keeps
     stderr; whether a terminal launch also writes the unified log is asked
     by no question.
   - **Unbuilt, and why.** The emitter needs three answers that are open:
     the route (question 2; Jev picked the C shim at 0.97 and 1.0, held as
     conditional because it would be the first C object in the shipping
     binary), the type `warn` takes (question 4; 0.12 and 0.6) and each
     value's privacy, which is the shim's format specifier (question 5;
     0.27 and 0.22). How the level is set (question 6; 0.24 and 0.4) is open
     too. Telling a launched baud from a terminal launch, by fd 2 being
     `/dev/null` (§1.3), needs none of them, but on its own it changes nothing
     a user can see: the line still has nowhere to go. So the choice of sink
     lands with the emitter, with its unit test (the unified log is chosen
     when stderr is `/dev/null`) red first, and the manual check that a
     launched baud's `warn` line is found by `log show --predicate
     'subsystem == "dev.baud.terminal"'`.
2. **If the unified log, by which route?**
   - **A C shim.**
     - For: it stays on the documented surface; it gives subsystem and
       category, per-value privacy and all five types; it builds with the
       shell's `cc` (M).
     - Against: it is the first C object in the shipping binary, compiled by
       `baud-app/build.rs`; its types need the C-probe test.
     - Cost: measured, as `os_log_with_type` from C (§4).
   - **`syslog(3)`.**
     - For: no C; one more libc call in `sys.rs`, as the existing ones are;
       its lines reach the unified log (M).
     - Mark: a documented API that Apple's Logging page says unified logging
       supersedes ([[source: apple-logging]]); not the current guidance.
     - Against: Apple says unified logging supersedes it; no subsystem, so
       readers filter by process; `LOG_CRIT`, `LOG_ALERT` and `LOG_ERR` all
       land as Default (M; `LOG_EMERG` untested), so no tested priority
       reaches Error; os_log's privacy annotations lose the value (M), so a
       value is shown whole or left out.
     - Cost: not measured.
   - **A buffer built in Rust for `_os_log_impl`.**
     - For: no C, and the full feature set if it holds.
     - Against: it rests on an undocumented compiler encoding (a departure);
       it needs `#[link_section]`, an unsafe attribute, and `__dso_handle`.
     - Cost: not measured.
3. **Linux:**
   - the journal's native protocol (conforming: journald's documented
     interface; cost not measured);
   - stderr (conforming only for service units, by journald's texts; GNOME
     routes it at info, as practice; cost as today's `eprintln!`);
   - or a file under `$XDG_STATE_HOME/baud` (conforming: the XDG spec says
     state "may contain … logs"; cost as (b) in question 1)?
4. **`warn` as Default or as Error?** Apple's Default "Captures information
   that is essential for troubleshooting problems. For example, capture
   information that might result in a failure." Its Error "Captures errors seen
   during the execution of your code." Both are persisted "Yes, up to a storage
   limit". The precedents differ:
   - ghostty maps warn to Error;
   - kitty sends every line as Error (`os_log_error`);
   - iTerm2's `ELog` reaches the unified log through `NSLog`, which landed as
     Default (M, [[source: ai-obs-stderr-marker-requery]]).
   - **Default:** conforms to Apple's Default text above. **Error:** conforms
     if a refusal counts as "errors seen during the execution of your code";
     that judgement is the owner's.
   - Cost of both: a 125 ns median through the shim; in the burst, Error's
     slowest call was 47 to 64 us and Default's 68 to 115 us (§4). Apple says
     "Faults and other severe messages incur more overhead"
     ([[source: apple-generating-log-messages]]).
5. **Privacy: may link URIs and home paths go out `%{public}s`?**
   - os_log(3) recommends "that log messages consist strictly of static strings
     and numbers". OSLogPrivacy says to "use the privacy options to hide
     potentially sensitive information".
   - Public URIs and home paths depart from both. `%{private}s` (redacted
     unless a profile enables private data) or leaving them out conforms.
   - Sizes and counts are numbers and conform as public.
   - Through `syslog(3)`, only showing or omitting is possible (M).
   - Cost: public values sit in a store an admin can read; `%{private}s`
     redacts the very path a reader needs to act on; leaving a value out loses
     it.
6. **How is the level set for a launched baud?**
   - a config key (baud's own; no platform text);
   - `LSEnvironment` in the bundle (a documented Info.plist key, in a 2018
     archive reference; fixed when the bundle is built);
   - `launchctl setenv BAUD_LOG …` (documented, but under launchctl(1)'s
     LEGACY SUBCOMMANDS);
   - or, under the unified log, the system's own levels. `log config` and a
     subsystem profile are documented "while debugging in macOS"
     ([[source: apple-customizing-logging]]), a debugging tool rather than a
     user setting. `log stream --level` is documented in log(1), and is live
     only.
   - In every case `BAUD_LOG` stays for stderr.
   - Cost: a config key is new code that runs before `log::init`;
     `LSEnvironment` is fixed per bundle build; `launchctl setenv` applies to
     "all future processes launched by launchd in the caller's context"
     ([[source: macos-man-launchctl]]); `log config` needs root, and
     `log stream` keeps nothing.
7. **The budget line and `placements_refused`.** Should the budget line stay
   `info`, or rise so that it is persisted? Should `placements_refused` get a
   `warn` line by the refusal line's rule: once a pump, naming the limit hit?
   - Info conforms to Apple's Info text ("Captures information that is
     helpful, but not essential, to troubleshoot problems") and is persisted
     only when collected with the log tool. Default conforms only if the
     budget counts as "essential for troubleshooting". Cost: one persisted
     line a launch (§6).
   - The `placements_refused` line takes question 4's level; its cost is the
     refusal line's, once a pump (not measured).
8. **Practice only:** should a warning also show in the window, as alacritty's
   message bar does? Cost: §5(e)'s renderer surface, unbuilt.

## 8. Found on the way, decided by no question

The first two are fixed (2026-10-09, the record, "`BAUD_LOG` governs
the trace lines, and the Linux driver says what is still unverified"); the
rest are open.

- **Fixed: `trace: off (set BAUD_TRACE=<prefix> to record the session)`
  printed on every launch** through a bare `eprintln!` (app.rs:893). It
  ignored `BAUD_LOG`, so `BAUD_LOG=err` did not silence it (C). It belonged
  behind `info!`, and is there now, with `trace: writing …`; `trace:
  BAUD_TRACE=… but the files could not be created` is `warn!`, as `BAUD_GPU`'s
  unreadable value is. The nine `eprintln!` sites under their own variables
  (§1.2) are unchanged.
- **Fixed: linux.rs:101 said at `warn`, on every launch, that the driver had
  "not yet run on a compositor".** CLAUDE.md says the Linux port "has run only
  headless", and calls that "the headless weston run", so the sentence was
  stale (C). It now says the driver has run only on a headless compositor,
  that nobody has looked at its pixels or typed into it, and that it has no
  pointer.
- **`placements_refused` is counted and never said.** The record already
  lists it as open (§1.4). If it takes the refusal line's form, no question
  decides it; if its level is in doubt, it is question 7.
- **For any route through the unified log, look up the log object once.**
  Later calls return the same object, so ghostty's per-call `os_log_create`
  and release are balanced. But the header calls per-operation creation
  "generally inappropriate", and [[source: apple-os-log-create]] says: "You
  only need to call this function once to initialize a custom log object."
