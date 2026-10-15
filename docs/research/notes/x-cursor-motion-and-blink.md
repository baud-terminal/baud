# X. The cursor's motion and blink: the designs as reviewed

Written 2026-09-27, read against `1ce14c8` and `8fab372`. **These are the designs
the builds of that day were given**, kept so the reasoning behind each choice
outlives the session scratchpad it was written in. Each was drafted by one
agent and refuted by a second, and the version here is the corrected one.
Names of tests that were proposed and never written, and paths that exist
only in that scratchpad, appear without code formatting, so this note does
not cite what the tree lacks.

The owner's answers of 2026-09-27: blink-after-exit rule **F**; ride-scroll
**rides at most two rows, then folds in** on the fast constant; the blink from
the OS **with hard edges at 650/350**; and **every other sub-question at its
recommended default**. What was built, and where the build departed from a
design, is recorded in `CLAUDE.md` under each feature's own section:

| design | built | section |
|---|---|---|
| Reduce Motion | `336d269` | "Reduce Motion means no trail, unless the config names one" |
| blink from the OS | `9c4769d` | "The cursor blinks at the system caret's rate" |
| rule F for `?12` | `2b54cfd` | "`?12` is a second input to the blink" |
| ride-scroll | the commit after `a349d7e` | "A trail rides a smooth scroll, two rows at most" |


## Reduce Motion

## Reduce Motion means no trail, unless the config names one (reviewed and corrected)

Build: "reduce-motion". The owner's decision is at docs/research/notes/v-open-decisions.md:50: **"Reduce Motion means no trail, unless the config names one."** [read]

Labels:
- **[read]**: read at source.
- **[run]**: run by me, in `scratchpad/cursorq/rv/`, the probes `order.m`, `callee.m`, `regcost.m`, `stage.m` and `sub.m`.
- **[inferred]**: reasoned from what was read or run.

Nothing in the repository was edited, and no cargo command was run there.

### Verdict

**What survives:**
- `TrailPick`, and the pure `effective` rule.
- `Chase::restyle`, and finding E, the trail that owes a frame forever.
- `BAUD_MOTION`.
- The config table.
- No change to the renderer.
- The idle story.

**What does not:** the centrepiece. The design says a `DisplayOptionsObserver` in macos-liquid-glass is "required for correctness", because nothing would otherwise invalidate AppKit's cached byte. **My probes refute that. AppKit registers its own observer inside `NSApplication.sharedApplication`.** So the design needs no crate change, no crate release, no `unsafe` registration, no launch-order constraint and no main-thread crate test. The build polls the safe getter on the tick, beside `poll_focus`, which is the house precedent. Eight smaller corrections follow in section 1.

---

### 0. The design's findings, re-checked

**D. The config cannot tell a named trail from the default.** Confirmed [read].
- The default is `CursorTrail::Fire`: config.rs:228-242 and :308.
- The reader overwrites the field and keeps no trace that the key was present (config.rs:393-413). `"fire" | "true"` both give `Fire` (:395), and an unrecognised value gives `Fire` with "using fire" (:406-411).
- The example names the trail: `trail = "fire"` at config.toml.example:102.
- The example gate asserts `CursorTrail::Fire` (config.rs:1372-1377).
- The owner's `~/.config/baud/config.toml` has only `[glass]` (:39) and `[images]` (:84) [run: grep].

**E. Switching the trail to off mid-flight owes a frame on every tick.** Confirmed [read].
- `present` handles `Off => None` and never calls `chase.frame` (app.rs:1032-1062).
- `owes` returns `Frame` while `moving` or `fire` is set (trail.rs:302-312). Those are cleared only in `frame` (trail.rs:127-221) or in `snap` (trail.rs:316-322).
- The tick forces a frame on `owes` (app.rs:3879).

**C. `baud-app` cannot register an observer.** Confirmed [read].
- app.rs:2 is `#![forbid(unsafe_code)]`.
- Both registration calls are `pub unsafe fn`: objc2-foundation-0.3.2 src/generated/NSNotification.rs:145 and :212.
- The notification name is an `extern "C"` static (objc2-app-kit-0.3.2 src/generated/NSAccessibility.rs:188-191).
- The getter itself is safe: macos-liquid-glass-1.0.0-beta.5 src/accessibility.rs:80-87.

**B. The cache and its invalidation.** Upgraded from [inferred] to **[run]**, and its conclusion is **refuted**.
- **The getter caches one byte and reads from HIServices.** The getter caches `__MergedGlobals+0xd0b` (appkit.dis:588556-588572) [read]. It calls HIServices `_AXInterfaceGetReduceMotionEnabled` [run: callee.m follows the stub]. That function is not cached. It reads `_kAXInterfaceReduceMotionKey` through a preferences call, unless a private override is set (hiservices.dis:5962-5990) [read].
- **The handler invalidates, then posts.** In `_accessibilityDisplaySettingsDidChange:`, the first stub sends `_invalidateAccessibilityDisplayValues` to the `NSWorkspace` class. The second sends `_checkForObserversOfSubsystem:` with 0x10. Then it posts the notification [run: callee.m decodes both selector strings].
- **The invalidation is visible inside the post** [run: order.m].
  - I swizzled the invalidation with a counter and called the handler.
  - With no observer, the handler still invalidates (count 1).
  - With an observer, the count is 2 when the observer's block runs, so the invalidation happens before the post.
  - An observer on `NSNotificationCenter.defaultCenter` hears nothing (0 posts). That confirms the header, NSWorkspace.h:33, where observers "must be registered on this notification center" [read, SDK 27].
- **The listener already exists at launch** [run: stage.m, sub.m]. This is the refutation.
  - Before `sharedApplication`, `hasObserversForNotificationName:` is 0 and `_subsystemContexts[16]` is null.
  - **After `sharedApplication` it is 1, with the subsystem live.** Nothing of mine had registered.
  - So AppKit keeps the display-options subsystem alive itself, and every OS toggle reaches the invalidation whether or not baud observes.
- **Consequences:**
  - The "observer is required for correctness" conclusion falls.
  - So does the "register before the first read" ordering, and its source gate.
  - Risk 2 falls too (a toggle during launch missed): a read after `sharedApplication` (app.rs:4082) is always invalidated.
- **Costs** [run: regcost, six runs, after `sharedApplication` and one window]:
  - registration: 1.0-1.6 µs;
  - first read: 21.8-29.5 µs;
  - second read: 0.000-0.042 µs.
- **The subsystem is not what the design described.** `_createAccessibilityDisplaySubsystem` makes at least eight distributed registrations, each with `w6 = 4` (deliver immediately), not four (appkit.dis:9098-9175) [read].

---

### 1. Corrections to the design

1. **Poll on the tick, and drop the observer from the default design.** The source is `macos_liquid_glass::accessibility::reduce_motion()`, and the reason is finding B above.
   - It sits beside `poll_focus()` under `Trigger::Tick` (app.rs:3348-3349). That precedent polls an AppKit getter once a tick because a crate hook did not fire (app.rs:4434-4449) [read].
   - The cost is one message send and one cached byte a tick. That is about 0.04 µs [run, the getter alone]; with `sharedWorkspace` I infer under 0.1 µs [inferred]. At 120 Hz it is under 12 µs a second, about 0.001% of a core, and under a fortieth of row 11's instrument floor of 0.044-0.060% [read: CLAUDE.md, the idle section].
   - It builds no frame.
   - The observer moves to a fallback (owner question 6).
2. **The CPU path's clearing frame is already gated.** The design called this [inferred]; it is [read].
   - `trail_moved` compares `under_trail(trail)` with `under_trail(last_trail)` for any `Drawn` (renderer.rs:1400-1401).
   - `moving_smear`'s step at 0.5 s sets the look to `None` and requires the incremental frame to equal the full one (cpu_overlays.rs:389-424).
   - `the_cpu_path_draws_an_effect_as_its_emitter` ends with `set_trail(None)`, incremental, and "gone, it leaves nothing" (cpu_overlays.rs:486-490).
   - Drop that risk. Nothing new is owed.
3. **Only one switch is reachable.** The effective trail moves only for `TrailPick::Default`: fire, or the quad on the CPU path through `chase_style` (app.rs:116-123), against off. A named ghost never changes, so `retrail`'s ghost clear is unreachable today.
   - Keep the clear anyway. It is the screen switch's idiom, `chase.snap(); last_cursor_cell = None; trail = None` (app.rs:3475-3477), and it becomes reachable if the default ever becomes the ghost.
   - Drop the perturbation "retrail leaves the ghost": it drives a state no stream reaches.
4. **`App::set_motion` cannot be unit-tested, because it is AppKit-bound.** The decision has to be pure, and the "exactly one frame" claim needs a pure function behind it (section 6).
5. **A missed compile break.**
   - `the_cursor_page_shows_every_trail` (config.rs:1014) calls `trail_name(Config::defaults().cursor_trail)` and asserts `read(..).cursor_trail == t`. Both break under `TrailPick`.
   - The same gate parses the refusal text by splitting on `"is not "` and `;`. The new "using the default" wording must keep the `;`.
6. **The observer cannot be built in `App::new` as written.** `App::new` takes no `MainThreadMarker` (app.rs:680). This matters only for the fallback. There, build it in `run()` beside the style observer and pass what it needs into `App::new`.
7. **Stale line numbers.** The TODO line is TODO.md:410-411, not 388-389 [read]. `Boot::prepare` is at app.rs:4034, `sharedApplication` at :4082, `GlassWindow::new` at :4105 and `App::new` at :4167.
8. **docs/cursor.html has no Reduce Motion sentence to "update"** (its only matches are the page's own `prefers-reduced-motion` CSS, :210 and :371). One must be added. Its lede also still says "Liquid is wanted and not built yet" (:221). That is stale, separate, and one line to fix in the same commit.

---

### 2. State

| where | state | kind |
|---|---|---|
| config.rs | `enum TrailPick { #[default] Default, Named(CursorTrail) }` | type |
| config.rs | `enum SystemMotion { Full, Reduced }`; the name `Motion` is taken in app.rs:13 by `reveal::Motion` [read] | type |
| config.rs | `TrailPick::effective(self, SystemMotion) -> CursorTrail`: `Named(t)` gives `t`; `Default` gives `CursorTrail::default()` under `Full` and `Off` under `Reduced` | pure fn |
| `Config` | `cursor_trail: TrailPick` | config |
| `Boot` (app.rs:508) | `cursor_trail: TrailPick` | carried |
| `App` (app.rs:465) | `trail_pick: TrailPick`, `motion: SystemMotion`, `motion_source: MotionSource`. `cursor_trail: CursorTrail` stays and now means **the effective trail** | runtime |
| app.rs | `enum MotionSource { System, Fixed(SystemMotion) }`, read once from `BAUD_MOTION` at launch; no `bool` | runtime |
| trail.rs | `Chase::restyle(&mut self, style)`: `*self = Chase { next_seed, last_lit, ..Chase::new(style) }` | method |

- Gate `effective` and `SystemMotion` with `#[cfg(any(target_os = "macos", test))]`, as `CursorTrail::style` is gated (config.rs:248) [read].
- The Linux lint's view of `Named(CursorTrail)`'s field is [inferred] benign. Today's `pub` field `cursor_trail` is never read on Linux, and `test:linux` passes with `-D warnings` (devenv.nix:300) [read]. The build runs the cross-lint to confirm.
- **Why `restyle` and not `Chase::new`:** `Chase::new` sets `last_lit` to `None` (trail.rs:117). That lets a fourth ignition light within a second, against the WCAG guard (`REIGNITE`, trail.rs:63, 266-277). `snap` keeps `next_seed` and `last_lit` for the same reason (trail.rs:316-322) [read].

### 3. Where each piece lives

**macos-liquid-glass: nothing.** baud already calls `accessibility::reduce_motion()`, which is safe. No release is needed.

**baud-app/src/config.rs:**
- `TrailPick`, `SystemMotion` and `effective`.
- The reader arm (:393-413):
  - named values give `Named(..)`;
  - `"off" | "false"` gives `Named(Off)`;
  - `"true"` gives `Default` (owner question 1);
  - an unrecognised value gives `Default`, with "…; using the default", keeping the `;` (owner question 2).
- Doc comments at :57-62 and :221-227.

**baud-app/src/trail.rs:** `Chase::restyle`.

**baud-app/src/app.rs:**
- `fn motion_source() -> MotionSource`, the parse of `BAUD_MOTION=reduced|full`, beside `scroller_style` (app.rs:5272). It is a pure `motion_override(Option<&str>)`.
  - `BAUD_SCROLLER` is the precedent, "for a check that must not change the owner's settings" (app.rs:5266-5277) [read].
- `fn system_motion(src) -> SystemMotion`. `Fixed(m)` gives `m`. `System` reads the getter.
  - The getter must be read after `sharedApplication`. `App::new` is after it (app.rs:4082 then :4167), so the launch read is safe.
- **`Driver::poll_motion()`**, called under `Trigger::Tick` right after `poll_focus()` (app.rs:3349), before `apply_style` (:3371):
  - `Fixed` returns at once;
  - otherwise it reads `system_motion`;
  - if the answer equals `self.app.motion`, it returns;
  - otherwise it stores the answer, logs `motion: reduced|full; trail: <effective>` at info, and applies `motion_change`.
- **`fn motion_change(pick, current: CursorTrail, m) -> Option<CursorTrail>`**, pure: `Some(next)` only when `pick.effective(m) != current`.
- **`fn retrail(chase, ghost, last_cell, next, path, now) -> Owes`**, pure. It:
  - calls `chase.restyle(chase_style(next.style(), path))`;
  - sets `*ghost = None` and `*last_cell = None`;
  - returns `Frame` only if the chase owed a frame at `now` before the restyle, or a ghost was in flight.

  The driver sets `cursor_trail = next`, and sets `pending = Dirty` only on `Frame`. So a toggle with nothing drawn builds no frame.
- **Ordering inside the tick** [read]:
  - `chase.owes` is evaluated at app.rs:3879, before `pump(Trigger::Tick)` at :3910.
  - A switch landing on that tick builds its clearing frame in the same tick: `present` runs at the end of `pump`, because `due` holds on `Tick` (app.rs:3535-3541).
  - If the GPU is busy, `pending` stays `Dirty` and the next tick or wake draws it.
- **Launch.** In `App::new`:
  - read `motion_source()`, then `system_motion()`, with `startup_mark("reduce motion")`;
  - compute `cursor_trail = pick.effective(motion)` before `Chase::new` (app.rs:880);
  - log the value at info, not only through `startup_note`, which prints under `BAUD_STARTUP` only (host.rs:180-185).
- **Linux: nothing.** linux.rs and host.rs never read the trail [read]. The portal key is recorded as not built.

### 4. Config keys

**No new key.**

| config | Reduce Motion off | Reduce Motion on |
|---|---|---|
| no `trail` key (the owner's config) | fire | **off** |
| `trail = true` (owner question 1) | fire | **off** |
| unrecognised value (owner question 2) | fire, with a complaint | **off**, with a complaint |
| any named trail | that trail | **that trail** |
| `off` / `false` | off | off |

- `scripts/trail-clips.sh:65` writes `trail = "%s"`, so every clip is named and unaffected [read].
- On the example line (config.toml.example:102), see owner question 3.

### 5. What the renderer is handed

**Nothing new.** Under an effective `Off`, `present` hands `set_trail(None)` (app.rs:1062-1064), the path `trail = "off"` takes today. The clearing frame is gated on every path:
- the CPU path, per correction 2;
- the GPU paths, which rebuild the trail's records every frame.

A frame stays a pure function of what the driver hands the renderer. baud-render and baud-core are untouched.

### 6. Idle behaviour

- **An idle screen builds no frame under either setting.**
  - Under Reduce Motion, `present` never calls `chase.frame`, and the chase, restyled or built at rest, owes `Nothing`. A ghost needs `cursor_trail == Ghost` (app.rs:1001).
  - This is the `trail = "off"` path exactly [read].
- **The new per-tick work is one getter read**, about 0.04 µs [run]. It sits beside `poll_focus`'s `isKeyWindow`. It is below any floor this repository keeps, and no sweep is owed.
- **A toggle of another display option** re-reads the setting, finds it equal and returns. That covers Increase Contrast, Reduce Transparency and Invert Colors, which share the notification and so the invalidation. It costs about 22 µs once, for the uncached read, and no frame.
- **A switch mid-trail costs one frame. A switch at rest costs none.**

### 7. Tests, each red first, at every level that exists

**There is no recorded level.** No recording carries an OS setting. That is the precedent of the HT and SO/SI fixtures.

**L1. Config** (config.rs; every platform)
- `reduce_motion_turns_the_default_off_and_leaves_a_named_trail`: `Default` and every `Named`, crossed with `Full` and `Reduced`. Red against an `effective` that ignores the motion.
- `the_cursor_trail_is_the_plasma_unless_named` (config.rs:916-955), updated:
  - named values map to `Named(..)`;
  - `true` and `yes` map to `Default`;
  - `yes` still produces one complaint.
  - Red against the old reader.
- `the_cursor_page_shows_every_trail` (config.rs:1014), updated:
  - `trail_name(CursorTrail::default())`;
  - `read(..).cursor_trail == TrailPick::Named(t)`;
  - the refusal-list parse still passes.

  This is a compile break today and must land in the same commit.
- `the_committed_example_parses_to_what_it_documents` (config.rs:1352-1401), per owner question 3. If the line is commented out, the trail assertion becomes `TrailPick::Default`, a default like its neighbours. The `said.is_empty()` check and the key-line count still show that `[cursor]` landed through `shape` and `blink`.

**L2. The trail driver** (trail.rs; every platform)
- `a_restyle_mid_fire_owes_nothing`:
  - a 30-column move lights a fire;
  - `restyle(Style::Smear(Shape::Quad))`;
  - `frame` is never called again, as under `Off`;
  - `owes` must be `Nothing` on all 1,200 ticks over 10 s at 120 Hz.
  - Red against a restyle that only sets `style`: `Frame` on all 1,200.
- `a_restyle_keeps_the_ignition_guard_and_the_seed`:
  - fire at t0, restyle to the quad and back to fire;
  - a first sighting, then a long move after the 10 ms dwell but inside `REIGNITE`: no ignition;
  - after `REIGNITE`, the next ignition takes seed 2.
  - Red against a restyle built from `Chase::new`.
- `a_restyle_at_rest_owes_nothing`, the control.

**L3. The app's pure pieces** (app.rs tests, macOS; the shape of `helix_trails_on_ge_and_gg` at app.rs:2805)
- `motion_change_follows_only_the_default`:
  - `Default` from `Full` to `Reduced` gives `Some(Off)`, and back gives `Some(Fire)`;
  - every `Named` gives `None` both ways;
  - an unchanged effective trail gives `None`.
  - Red against "the motion is ignored".
- `a_switch_mid_fire_owes_one_frame_and_then_none`:
  - `retrail` on a burning fire returns `Frame`;
  - the restyled chase then owes `Nothing` over 10 s of ticks.
- `a_switch_at_rest_owes_no_frame`: `retrail` on a chase at rest returns `Nothing`.
- `the_motion_override_reads_reduced_and_full`: `reduced`, `full`, unset, and garbage, which gives `System`.
- `a_fixed_motion_never_asks_the_system`: `system_motion(Fixed(Reduced))` returns `Reduced` without calling the getter. The read is passed in as a closure, so the test can count calls.
- An optional source gate, in the shape of `every_reader_of_the_window_asks_the_clamp` (app.rs:2565): `pump`'s `Trigger::Tick` arm calls `poll_motion`. `pump` is AppKit-bound, so this is the only thing that can fail on a removed call.
- The rest of the wiring is established by reading: `poll_motion`'s body, `App::new`'s launch read, and `present`. This follows the convention the cursor-blink section records for driver wiring.

**L4. Live** (the owner): see M3. This is the only check that the OS toggle reaches AppKit's handler, and that the re-read is fresh.

### 8. Perturbations: one mechanism each, each red at its own test

| broken on purpose | goes red |
|---|---|
| `effective` ignores the motion | L1 table, and L3 `motion_change` |
| `"true"` read as `Named(Fire)` (if owner question 1 is taken) | L1 reader |
| an unrecognised value read as `Named(Fire)` (if owner question 2 is taken) | L1 reader |
| the complaint loses its `;` | L1 page gate |
| the example's line restored (if owner question 3 is taken) | L1 example gate |
| `restyle` only sets `style` | L2 mid-fire, and L3 switch |
| `restyle` resets `last_lit` | L2 guard |
| `restyle` resets `next_seed` | L2 seed assertion |
| `retrail` always owes a frame | L3 at rest |
| `retrail` never owes a frame | L3 switch |
| `motion_change` fires for a named trail | L3 `motion_change` |
| `motion_override` misreads `reduced` | L3 override |
| `system_motion` consults the getter under `Fixed` | L3 fixed |
| `poll_motion` removed from the tick | the source gate, if taken |

Run each perturbation with `--no-fail-fast`. Re-spell any perturbation that stops compiling under `-D warnings`. Both rules are recorded in CLAUDE.md.

### 9. Measurements owed

- **M1. Launch** (`scripts/startup-bench.py --rounds 7 --arm base=… --arm new=… --aa`, on mains). Prediction: every stage inside the A/A spread. The one new launch cost is a 22-30 µs read [run].
- **M2. End to end, without touching settings** (`scripts/idle-cpu.py` with the fire section's protocol and an A/A copy). Frame counts come from the app's own report:

  | arm | prediction, frames per 10 s |
  |---|---|
  | `BAUD_MOTION=reduced`, trail from the default | about 48-50 (the `off` figure) |
  | `trail = "off"` | about 48-50 |
  | `BAUD_MOTION=full`, trail from the default | about 1,181-1,184 (fire) |
  | `BAUD_MOTION=reduced` with `trail = "fire"` | about 1,181-1,184 |

  The frames are the result. The CPU column is none, under a 120 Hz animation.
- **M3. The live toggle** (the owner; I changed no setting). Run baud with the mover and `BAUD_LOG=info`, and turn System Settings › Accessibility › Display › Reduce motion on, then off. Check that:
  - the `motion:` line appears within a tick and names the **new** value (a stale re-read would name the old one);
  - the frames fall to the off figure, then return;
  - toggling Increase contrast logs nothing.
- **Owed by no one.** baud-core and baud-render are untouched, so no `ingest2`, `frame` or `frame_gpu` sweep is owed.

### 10. Docs, in the one baud commit

- **CLAUDE.md:**
  - add a section "Reduce Motion means no trail, unless the config names one", with the probes' findings and labels, the poll and why the observer was not needed, the restyle, and M1-M3 as they land;
  - strike "Reduce Motion" from the three "Not built, stated" lists (CLAUDE.md:19729, :19869, :20044).
- **docs/cursor.md:236-237**, and add the sentence to docs/cursor.html. Fix the stale liquid lede at cursor.html:221 while there.
- **config.toml.example:80-102**, per owner question 3.
- **config.rs** doc comments at :57-62 and :221-227.
- **docs/research/notes/v-open-decisions.md**, the "Built since" paragraph.
- **TODO.md:410-411**, to be ticked. The file is gitignored; never commit it.
- **The app.rs field doc at :459-465**, which still says the trail is "a fading ghost".

### 11. Not in this build

- Smooth scroll, the screen fade, the scrollbar fade, the bell flash and the caret blink (owner question 5).
- The Linux portal key.
- The crate observer, unless M3 or owner question 6 calls for it.


### Owner questions
- Does `trail = true` name a trail? Recommended: no. The docs call `true` the default (config.rs:226-227, config.toml.example:96), so under Reduce Motion it gives off.
- Does an unrecognised `[cursor] trail` value name a trail? Recommended: no. It falls back to the default, so it is off under Reduce Motion, and the complaint says 'using the default' (keeping the `;` the page gate splits on). A typo should not override an accessibility setting.
- config.toml.example:102 writes `trail = "fire"`, so anyone who copied the example keeps a trail under Reduce Motion. Recommended: comment the line out and add one sentence saying that naming a trail keeps it under Reduce Motion.
- What happens to a trail or fire in flight when Reduce Motion turns on? Recommended: cut it at once, with the one frame that clears it, rather than letting it burn out (up to about 0.94 s).
- Scope. Recommended: the trail only, as decided. Smooth-scroll slides, the alternate-screen fade, the scrollbar fade, the visual bell and the caret blink stay unchanged until asked, though NSAccessibility.h:102 asks apps to avoid 'large animations'.
- Poll or observe? Recommended: poll `accessibility::reduce_motion()` on the tick, like `poll_focus`. It is safe, costs about 0.04 µs a tick, and needs no crate change. It relies on AppKit's own observer, which `sharedApplication` registers [run on this OS]. The fallback is an observer in macos-liquid-glass (selector form on the workspace centre, hopping to the main thread), which does not rely on that internal. It needs a crate release, a main-thread test harness the crate lacks, and construction in `run()`, not `App::new`. Take it only if M3 shows a stale read, or if you prefer not to depend on AppKit's internal.
- A `BAUD_MOTION=reduced|full` override, read once at launch, so checks can run without changing your settings, with `BAUD_SCROLLER` as the precedent. While it is set the poll is skipped. Recommended: yes.
- The Linux portal key `org.freedesktop.appearance reduced-motion`. Recommended: out of scope and recorded as not built, since the Linux driver draws no trail.

### Risks
- Polling rests on an AppKit internal. `NSApplication.sharedApplication` registers its own observer of the display-options notification, which keeps the subsystem and its invalidating handler alive. This was run on this OS (Darwin 27 / SDK 27) and is not documented. If a later macOS drops that observer, the cached byte is never invalidated and a toggle is missed until relaunch. M3 detects this now. The crate observer (owner question 6) is the hedge, since registering creates the subsystem itself.
- Whether the fresh read is fresh is inferred. The getter reads HIServices `_AXInterfaceGetReduceMotionEnabled`, which reads a preferences key after AppKit invalidates. If the in-process preferences cache lags the distributed notification, the re-read re-caches the old value, and nothing invalidates it until the next toggle. The same risk applies to the observer design. Only M3's check that the logged value is the new one can show it.
- M3 also confirms that the OS toggle reaches `_accessibilityDisplaySettingsDidChange:`. The invalidate-then-post order is run (order.m, callee.m). The delivery from System Settings to that handler is not.
- Switching the trail mid-flight is the main correctness risk. If `moving` or `fire` is left set, a frame is owed on every tick forever (finding E). A chase rebuilt with `Chase::new` resets `last_lit`, which breaks the three-a-second ignition guard. The restyle-mid-fire and guard tests are the gates.
- Linux lint: `effective` and `SystemMotion` must be cfg-gated like `CursorTrail::style` (config.rs:248). Whether `Named(CursorTrail)`'s field trips dead_code on the Linux non-test build is inferred to be fine, because today's unread `pub` field `cursor_trail` passes `test:linux`. The build must run the cross-lint.
- `the_cursor_page_shows_every_trail` (config.rs:1014) stops compiling under `TrailPick`, and its refusal-list parse depends on the complaint keeping `is not ` and `;`. It has to be updated in the same commit.
- `App::set_motion` and `poll_motion` are AppKit-bound and cannot be unit-tested. Only the pure `motion_change` and `retrail` decisions are gated. The wiring is established by reading, plus an optional source gate.
- Users who copied `trail = "fire"` from the example keep the trail under Reduce Motion. That is by design, but it may surprise them. Changing the example fixes only new copies.
- Pre-existing, not this build's to fix: `chase_style` keys on `wanted`, but `present` can fall back to a CPU presenter without changing `wanted` (app.rs:1113-1118). The effect then runs on the CPU path, where nothing draws it, and the tick owes frames for it.

## The blink from the OS

The repository is `.` at `1ce14c8`. It was read only: nothing in it was edited and no cargo was run there. The runs below are standalone `rustc` programs under `scratchpad/cursorq/review/` (`frames.rs`, `conv.rs`, `stop.rs`). A copy of this design is at `scratchpad/cursorq/review/corrected-design.md`.

Claims carry one of three labels:
- **[source]**: read at source;
- **[run]**: executed here;
- **[inferred]**: reasoned, not checked.

### A. What the review refuted or changed

Most of the draft survives:
- the AppKit keyframe reading;
- the per-key parse table;
- `doubleForKey` as the reader;
- `dlopen`/`dlsym` in `pty.rs`/`sys.rs` with no window-crate change;
- the three-state `blink` defaulting to `"auto"`;
- no renderer or `baud-core` change under Q1(a);
- the stop timer's semantics;
- the Q2 reading.

The Q2 reading is supported by the owner's own words: "the system's non-blinking-caret switch is honoured unless the config says otherwise" (`docs/research/notes/v-open-decisions.md:52-55` [source]), and `blink = false` does not say otherwise.

These findings change the design:

1. **The harness counts 19 frames, not 20 [run].** `frames.rs` ports `caret.rs`. At 600/300 its first ticks are 73 and 109, and it counts 22. At 650/350 they are **79** and 121, and it counts **19**.
   - The first `due` in frames_over_ten_seconds flips `was` from `No` to `Yes`, and that restarts the epoch at tick 1 (`caret.rs:98-100` [source]). So the hide lands on tick 79, not 78.
   - At 600/300 the last flip, tick 1189, is still inside 1200 ticks. At 650/350 the shown flip at tick 1201 falls outside.
   - So test 2 asserts **19**.
   - The draft's "tick 78" justification is wrong.
   - The idle claim becomes 19 against 22 in the harness (-14%), and a steady-state rate of 2.00 against 2.22 flips a second (-10%).
2. **The float-conversion warning is unsupported [run].** `conv.rs` compared `Duration::try_from_secs_f64(ms / 1000.0)` against `(ms * 1e6).round()` on every 0.1 ms step from 0 to 60,000 ms: 600,000 values, and none differ.
   - try_from_secs_f64 is the house idiom: `HostTime::of_secs`, `baud-domain/src/time.rs:79-82` [source].
   - It is total. It returns `Err` for NaN, for ±inf, for a negative, and for 1e305 s; 1e-12 s gives 0 ns [run].
   - The design uses it, and decides the default, `Steady`, the ceiling and the floor on the `f64` before converting, so the conversion cannot fail.
3. **Test 11 cannot fail as written [run].** The draft's tail-f test has output every 50 ms.
   - Output restarts the cycle shown inside every 650 ms on phase, so no flip ever happens.
   - `stop.rs` counts **0 frames before 5 s and 0 after**, both with a working stop and with one that output resets.
   - The draft's own expectation, "frames at the flips until 5 s", is false on correct code.
   - With output every 2 s, the working stop gives 9 frames before and 1 after; the stop output resets gives 9 and **31**.
   - Test 8's output arm has the same flaw.
   - The corrected tests use output sparser than a cycle.
4. **The gate threaded through `blinks()` is correctness by reading.** macOS has two literal call sites, `app.rs:1179-1184` (present) and `app.rs:3852-3856` (tick) [source].
   - If the tick site loses the gate, the tick owes a frame at every flip, and present then plans `Shown`. That is an idle frame train on a steady caret [inferred from `caret.rs:108-116`].
   - No test drives either site.
   - `Caret` already owns the cycle and the stop, so the gate moves into `Caret` too. Both `blinks()` sites stay byte-identical to today, and the unit tests cover what the tick owes.
5. **`set_cycle`'s restart cannot be observed.** Every read happens on a focus gain, or on the first tick. Before a gain the blink is `No` (`caret.rs:54-60`), so the first `plan` after it takes the `was` edge and restarts the epoch anyway (`caret.rs:98-100` [source]).
   - The perturbation "`set_cycle` not restarting → 9, 13" cannot fire.
   - `set_system` therefore does not restart, and the row is dropped.
   - Tests 9 and 13 put the flips relative to the first `plan` after the gain, which is the one-tick shift of item 1.
6. **A click restarts a stopped blink [inferred from `caret.rs:97-106`].** Once the stop has folded the blink to `No`, any move of `quiet_since` makes the next plan go from `No` to `Yes`. So after the deadline a click restarts the cycle shown, which is GTK's rule; before it, the click only moves the deadline.
   - Test 8's click arm runs before the deadline.
   - Q5 states what happens after it.
7. **The draft's input set missed three sites [source].**
   - `App::send` (`app.rs:926-931`) is also the path for the wheel on the alternate screen, which sends arrow keys (`app.rs:3757-3760`).
   - A press reported to a tracking child (`mouse_report`, `app.rs:3701`, `:3743`) never reaches a "press the grid takes" site.
   - A paste writes to the pty directly (`app.rs:5066-5077`).

   All three are named below.
8. **The draft misattributes Chromium's switch handling [source].**
   - Chromium re-reads the *keys* on `NSApplicationWillBecomeActiveNotification` (`defaults_utils.mm:15-34`).
   - It *observes* `AXPrefersNonBlinkingTextInsertionIndicatorDidChangeNotification` for the switch (`os_settings_provider_mac.mm:66-76`).
   - It calls `AXPrefersNonBlinkingTextInsertionIndicator()` fresh on every query (`:159-165`).
   - Both files are in `scratchpad/cursorq/src/chromium_chromium`.
   - The draft said Chromium re-reads the switch on activation.
   - Whether an un-observed call returns a changed value: **not found**. So the focus-gain re-read is unverified, and the owner-run live check below is what settles it.
9. **The draft's startup measurement could not see the read [inferred].**
   - A scripted launch gives a window that is frontmost and not key (`CLAUDE.md:19372-19374` [source]).
   - A read gated on a focus gain therefore never runs under `scripts/startup-bench.py`, so the A/A sweep reads flat whether the read costs nothing or 2 ms.
   - The first read moves to the first tick, beside the Dock-icon hook at `app.rs:3818-3822` [source], with a startup mark of its own, and every focus gain reads again.
10. **The fallback `dlopen` loads a framework for nothing on macOS 13 and 14.** The symbol is macOS 15+ (`AXSettings.h:33` in MacOSX.sdk [source]).
    - The plain fallback `dlopen` would load Accessibility.framework there for a symbol it does not have.
    - The read is gated on `NSProcessInfo` reporting 15 or later first.
11. **The C probe is shared across platforms (`sys.rs:153-181` [source]).**
    - `#include <objc/objc.h>` does not exist on Linux, so the new prints go under `#ifdef __APPLE__`.
    - The `dlopen`/`dlsym` externs go under `#[cfg(target_os = "macos")]`.
    - Using `objc2::runtime::Bool` makes a `sizeof(BOOL)` probe unnecessary. It is `repr(transparent)` over the platform's `BOOL` (`objc2-0.6.4/src/runtime/bool.rs:95-99` [source]), and it is re-exported through `macos_liquid_glass::objc2` (`macos-liquid-glass-1.0.0-beta.5/src/lib.rs:170` [source]).
12. **Two of the draft's perturbation rows cannot fire as stated.**
    - "`<= 0` accepted → 4 (a panic at `%`)": `Phase` clamps to the floor, so a zero never reaches `%`. Only test 3 goes red.
    - "no ceiling → 3 (a panic from `Duration`)": with try_from_secs_f64 an overflow is an `Err`, not a panic. It goes red at test 3's `Steady` assertion.
13. **Test 10's DECRQSS half is a pin, not a red-first test.** `baud-core` is untouched, so nothing in this change can move that answer. Its gate half duplicates test 7.
14. **The Linux driver has to carry the choice.** Only `cfg.cursor_blink` reaches `set_initial_cursor_style` there (`linux.rs:152-155` [source]).
15. **Citation and wording fixes:**
    - The "no recorded level for a window's position" precedent is `CLAUDE.md:17339`, not `:17125`.
    - The "blink" that `linux.rs:17` lists as missing is SGR 5 text blink, and it is still unwired (`TODO.md:592`). Reword it; do not delete it.
    - Q1's "exactly 650 ms of full opacity" means equal *mean opacity*: an opacity integral of 556.25 + 93.75 = 650 ms per 1 s cycle. The caret is fully opaque for 537.5 ms [inferred, arithmetic on the run keyframes in `verify/anim-default.txt`].
16. **A failed present can leave the caret hidden with nothing owed [source].** `caret.commit(caret)` is taken whether or not the present succeeded (`app.rs:1476-1482`).
    - Its comment says "the next flip -- at most `caret::ON` away -- draws the phase again".
    - That is false once nothing flips: a stop, the switch, a 60 s ceiling.
    - The shape predates this change for a focus loss, and this change adds three more ways to reach it. The comment is rewritten, and the case is listed as a risk.
17. **The committed example writes `blink = true` (`config.toml.example:101` [source]),** so a config copied from it is explicit and ignores the switch. Mitigation: warn once at the read when the config says `true` and the switch is on.
18. **A rate-override key is named in `TODO.md:421` [source] and not in the owner's record.** It came from a sweep, not from the owner, so it is an owner question (Q8).

### B. The corrected design

#### 0. Scope

This is one commit. It covers:
- the periods, read from `NSTextInsertionPointBlinkPeriodOn` and `...Off` and unset as 650/350;
- the switch, `AXPrefersNonBlinkingTextInsertionIndicator`, macOS 15+;
- the three-state `[cursor] blink`, `"auto"` by default;
- `[cursor] stop_blinking_after`, with never as the default.

Edges are hard, by Q1(a). `baud-core` is not touched, and no renderer, shader or `.metallib` changes.

#### 1. State, all in `baud-app/src/caret.rs`

The module is pure and compiled `any(macos, linux, test)` (`main.rs:30-31` [source]).

```rust
/// One phase, FLOOR..=CEILING by construction.
pub struct Phase(Duration);
pub enum Cycle { Blinks { on: Phase, off: Phase }, Steady }
impl Cycle {
    pub const APPKIT: Cycle; // 650/350, from_millis
    pub const XTERM: Cycle;  // 600/300, from_millis
    /// AppKit's reading, total, decided on the f64 in this order:
    /// NaN or <= 0 -> that phase's own default (from `unset`);
    /// on > CEILING -> Steady; off > CEILING -> CEILING;
    /// < FLOOR -> FLOOR; then Duration::try_from_secs_f64(ms / 1000.0),
    /// which cannot fail on what is left.
    pub fn of_defaults(on_ms: f64, off_ms: f64, unset: Cycle) -> Cycle;
}
pub enum Switch { Off, On }          // absent (macOS 13/14, Linux) reads Off
pub enum Gate { Free, Steady }
impl Gate { pub fn of(c: BlinkChoice, s: Switch) -> Gate } // Steady iff s == On && c != On
pub enum StopAfter { Never, After(Duration) }

pub struct Caret<M> {
    epoch: M, was: Blinks, drawn: Blink,          // as today, caret.rs:71-79
    cycle: Cycle, gate: Gate, stop: StopAfter,
    quiet_since: M,                               // the last input; output never moves it
}
```

The methods:

* **`new(now, cycle, gate, stop)`**: `quiet_since = now`.
* **`restart(now)`**: output. It moves `epoch` only, as today.
* **`input(now)`**: a key sent, a paste, a focus gain. It moves `epoch` and `quiet_since`.
* **`quiet(now)`**: a mouse press, and a wheel (Q5). It moves `quiet_since` only.
* **`set_system(cycle, gate)`**: replaces both fields and does **not** restart. The `was` edge restarts the cycle at the next plan whenever the blink resumes (A5).
* **`plan(now, blinks)`**: before the `was` edge, `blinks` is folded to `No` when any one of these holds:
  - `gate == Steady`;
  - `cycle == Steady`;
  - `stop == After(d) && now.elapsed_since(quiet_since) >= d`.

  Then the method is today's, with `cycle_offset` taking the cycle (`caret.rs:143-148`). A `Phase` floor keeps its `%` nonzero.
* **`due` and `commit`**: unchanged in shape (`caret.rs:108-122`).
* **`next_change(now, blinks)`**, used on Linux and in tests: with the same fold it returns `None` when the blink is effectively `No`. Otherwise it returns the earlier of the next flip and `quiet_since + d`.

`blinks(style, focus, cursor)` keeps its three inputs (`caret.rs:54-60`), so none of its call sites changes: `app.rs:1179-1184`, `app.rs:3852-3856` and `linux.rs:302-308`.

#### 2. Config, `baud-app/src/config.rs`

`cursor_blink: CursorBlink` (`:68-69`, `:310`, `:429-440` [source]) becomes `cursor_blink: BlinkChoice { Auto, On, Off }`.
- The reader accepts `"auto"`, `true` and `false`, quoted or bare. That is the house word: `theme` at `:370`, `shadow` at `:525` [source].
- Anything else warns and gives `Auto`.
- `BlinkChoice::power_on() -> CursorBlink` maps `Auto` and `On` to `Blinking`, and `Off` to `Steady`.
- `app.rs:652` and `linux.rs:152-155` take `choice.power_on()`, so the core powers on exactly as today, and DECRQSS and DECRQM answer as today.

A new key, `stop_blinking_after`, in seconds, becomes a `StopAfter`.
- `0` or absent is `Never`.
- Otherwise the value goes through `Duration::try_from_secs_f64`, and an `Err` (negative, NaN, inf) warns and gives `Never`.
- The name is Q5.

#### 3. The macOS reads

**The periods.** In `app.rs`, in safe code:

```rust
fn os_cycle() -> Cycle {
    let d = NSUserDefaults::standardUserDefaults();
    Cycle::of_defaults(
        d.doubleForKey(ns!("NSTextInsertionPointBlinkPeriodOn")),
        d.doubleForKey(ns!("NSTextInsertionPointBlinkPeriodOff")),
        Cycle::APPKIT,
    )
}
```

Both bindings are safe `pub fn`s: `standardUserDefaults` at `objc2-foundation-0.3.2/src/generated/NSUserDefaults.rs:78` and `doubleForKey` at `:182`. The `NSUserDefaults` feature is on by the crate's defaults, `Cargo.toml:392` [source].

**The switch.** `pty::prefers_steady_caret() -> Switch` is `#[cfg(target_os = "macos")]`, beside `display_scale` (`pty.rs:640-676` [source]).
- If `NSProcessInfo` reports below 15.0, it returns `Off` with no `dlopen`.
- Otherwise it does:
  - `dlopen(c"/System/Library/Frameworks/Accessibility.framework/Accessibility", RTLD_NOLOAD | RTLD_LAZY)`;
  - on null, a plain `RTLD_LAZY | RTLD_LOCAL` open;
  - `dlsym(h, c"AXPrefersNonBlinkingTextInsertionIndicator")`, cached in a `OnceLock<Option<extern "C" fn() -> objc2::runtime::Bool>>`.
- A null handle or symbol is `Off`.
- The externs go in `sys.rs` under `#[cfg(target_os = "macos")]`, typed with a handle newtype and a mode newtype. The audit counts no `c_int`, `f64` or pointer type (`impl_audit.py:3-21` [source]), so `baud-app`'s floor of 1 raw site and 4 signature primitives (`audit_all.sh:233-242`) does not move [inferred from the regexes].

**When it reads.** Neither read is on the launch path, and there is no observer (Q4).
- **The first read is on the first tick.** It goes at `app.rs:3818-3822` [source], after `startup_first("first tick")`, beside the Dock icon, with `startup_mark("caret os")`.
  - The first present is before `run()`, so it is not delayed.
  - Unlike a focus gain, the first tick happens in a scripted launch, so `startup-bench.py` sees the cost.
- **Then every `Gained` edge** in `set_focus` (`app.rs:4453-4461` [source]). `poll_focus` runs at the head of every `pump(Tick)` (`app.rs:3347-3350`) and before every mouse event (`:3657`).
- Each read calls `caret.set_system(os_cycle(), Gate::of(self.blink_choice, switch))`, then `caret.input(now)` on a gain. It logs one `info!` line (`caret: 650/350 ms, switch off, read in N us`); the default level is `warn`, `log.rs:3-4` [source].
- Once, when `blink_choice == On` and the switch is on, it warns: "`[cursor] blink = true` overrides the system's non-blinking caret".

#### 4. The drivers

**macOS** (`app.rs`):
* `App.blink_choice: BlinkChoice`, from the config.
* `Caret::new(host_now(), Cycle::APPKIT, Gate::Free, cfg.cursor_stop)` at `:875`. It is never drawn before the first read, because focus starts `Lost` (`:876`).
* `App::send` (`:926-931`) calls `input`. That covers typed keys and the wheel's arrow keys on the alternate screen (`:3757-3760`); the second is stated, and is part of Q5.
* `paste` (`:5066-5077`) calls `input`.
* `mouse` (`:3656`) calls `quiet` on any `Down`, before the scrollbar, link, selection and tracking branches. So a press a tracking child takes counts too.
* `scroll` (`:3712`) calls `quiet` (Q5).
* The output drain keeps `restart` (`:3394-3396`).
* Both `blinks()` sites are unchanged.
* The comment at `:1476-1482` is rewritten: a present that fails on the frame that stops the blink leaves it hidden until another frame draws (A16).

**Linux** (`linux.rs`):
* `Caret::new(Instant::now(), Cycle::XTERM, Gate::of(cfg.cursor_blink, Switch::Off), cfg.cursor_stop)` at `:178`, so `auto` blinks there. The choice is read there and not stored, since nothing on Linux re-reads.
* `Event::Key` calls `input` (`:274-278`).
* `Event::Focus(Gained)` calls `input` (`:283-287`).
* `feed` keeps `restart` (`:320-321`).
* The poll timeout already takes `next_change` (`:223-228`), which now carries the stop deadline.

**Precedence (Q2).** The gate sits after the application's style and changes only the drawing:

| `[cursor] blink` | switch | power-on (DECRQSS) | app sends `DECSCUSR 1` | drawn |
|---|---|---|---|---|
| `auto` (default) | off | blinking | blinking | blinks |
| `auto` | on | blinking | blinking | steady |
| `true` | on | blinking | blinking | blinks, with a warning |
| `false` | on | steady | blinking | steady |
| `false` | off | steady | blinking | blinks, as today |

#### 5. What the renderer is handed

The renderer is unchanged. It gets `set_cursor_blink(Blink::Shown | Blink::Hidden)` (`renderer.rs:913-915`), and `cursor_drawn` stays the one rule (`:921-923` [source]).

The phase is a pure function of the `Caret`'s state and the moment the driver hands in: `seen_at` on macOS (`app.rs:1173`), `Instant::now()` on Linux (`linux.rs:405`). There is no clock read in the renderer and no sleep. Holding hard edges needs only the two-valued `Blink` (`renderer.rs:46-50`).

#### 6. Idle

| state | frames the blink owes |
|---|---|
| focused, blinking, 650/350 | 2 flips a 1.0 s cycle, 2.00 a second against 2.22 at 600/300. **19** in the 10 s harness against 22 [run] |
| switch on under `auto`, or `false` with the switch on, or a `Steady` cycle | at most one frame, if the drawn frame was hidden, then none. The gate is folded in `plan`, so the tick's `due` owes nothing (A4) |
| stopped after the quiet time | one frame if hidden, then none. Output does not revive it |
| unfocused, or looking back | none, as today (`caret.rs:54-60`, `app.rs:2073-2075`) |

Neither read runs on a tick after the first, except the tick on which focus is gained. The macOS tick's `due` (`app.rs:3847-3860`) is otherwise unchanged. On Linux the poll wakes at the next flip or at the stop deadline, and never while steady or stopped.

#### 7. Tests, red first

The procedure is the house's:
- land the new signatures with bodies that encode the old rule:
  - `of_defaults` returns `XTERM`;
  - `Gate::of` returns `Free`;
  - `plan` ignores the gate and the stop;
  - `"auto"` is not parsed;
- run them, and see each fail at its assertion, not at a compile error;
- then implement.

**Unit (`caret.rs`, every platform):**

1. ~~A test of AppKit's unset cycle replaces `:196-208`: Shown at 0 and 649, Hidden at 650 and 999, Shown at 1000.~~ Since 2026-10-15 the unset cycle is 333 ms a phase (the owner's call; the record, "The cursor blinks 333/333, and its hidden phase is the plasma glow"), held by `the_unset_cycle_is_three_thirty_three_each_phase`: Shown at 332, Hidden at 333 and 665, Shown at 666.
2. `a_blinking_cursor_costs_two_frames_a_cycle_and_a_steady_one_none` asserts **19**, from 22 (`:184-193`). Its doc says the first plan restarts the epoch at tick 1.
3. `each_key_is_read_on_its_own_as_appkit_reads_it`:
   - `(400, 0)` gives 400/350, and `(0, 400)` gives 650/400;
   - 0, a negative and NaN fall back per phase;
   - 400.7 gives 400.7 ms exactly;
   - on 1e308 and on +inf give `Steady`;
   - off 1e308 and off +inf give the ceiling;
   - 1e-12 gives the floor.
4. `no_phase_is_zero`: an edge grid over both inputs (±0, a subnormal, ±inf, NaN, the floor and the ceiling each ±1 ulp) never gives a zero cycle.
5. `input_and_output_restart_the_cycle` and the_next_hide_is_a_whole_on_later, at 650 (from `:212-232`).
6. `the_next_change_is_the_next_flip`, at 650/350 (from `:283-293`).
7. `a_steady_gate_steadies_a_blinking_style` and `the_gate_is_the_switch_unless_the_config_says_true`:
   - a `Caret` with `Gate::Steady` plans `Shown`;
   - `Gate::of` gives Q2's table.
8. The stop timer:
   * `a_blink_stops_shown_after_the_quiet_time_and_owes_one_frame`;
   * `output_does_not_hold_off_the_stop`, with output **every 2 s**, sparser than a cycle. It expects frames before the deadline, at most one after, and none from then on; `next_change` is `None` after the deadline;
   * `a_key_after_the_stop_blinks_again_from_shown`;
   * `a_click_before_the_stop_moves_the_deadline_and_not_the_phase`, where `quiet` runs at 3 s with a 5 s stop;
   * `a_click_after_the_stop_restarts_the_cycle_shown`, which pins A6;
   * `the_next_change_is_the_stop_when_it_comes_first`;
   * `never_is_never`.
9. `a_new_cycle_takes_effect_at_the_first_plan_after_the_gain`: flips at plan0 + 1.0 s and + 1.2 s.

**Hand model: 120 Hz ticks over one `Caret`, in the shape of frames_over_ten_seconds.**

10. a_tail_f_session_stops_blinking_after_the_quiet_time: output every 2 s, no key, a 5 s stop. It reproduces `stop.rs`'s 9 + 1, and the arm with the stop removed gives 9 + 31 [run].
11. `a_steady_switch_costs_no_frames_over_ten_seconds`: zero frames. It runs through `due`, which is the tick's path.
12. `a_focus_round_trip_with_a_new_cycle`.

**Pin, not red first:**

13. `the_switch_steadies_the_drawing_and_not_the_answer`. A real `Term` powered on `BlinkingBlock` answers DECRQSS `SP q` with `1 SP q`, while a `Caret` with `Gate::Steady` plans `Shown`. It guards the "draw-only" claim against a later change.

**Config (`config.rs`):**

14. the_blink_is_auto_unless_configured replaces `:1115-1145`.
15. `stop_blinking_after_reads_seconds_and_zero_is_never`.
16. `the_committed_example_parses_to_what_it_documents` (`:1352-1380`). It is red until the example writes `blink = "auto"` and `stop_blinking_after = 0`.

**Platform (macOS-only unit tests):**

17. `the_steady_switch_resolves_on_macos_fifteen_and_later` is a positive control. When `NSProcessInfo` reports 15 or later, the symbol resolves; the value is not asserted.
18. `the_constants_are_the_platform_headers_own` (`sys.rs:255-266`) gains `RTLD_LAZY`, `RTLD_LOCAL` and `RTLD_NOLOAD`, printed under `#ifdef __APPLE__` in the shared probe.

**Recorded level: none.** A window's clock and focus are app state, and no recording carries them (`CLAUDE.md:17339`).

**Live check, owner-run.** A script cannot make the window key.
- Precondition [run, by `grep`]: no config sets `blink =`. Neither `~/.config/baud/config.toml` nor the repository's `config.toml` has a `[cursor]` section.
- Launch the bundle as `open -a baud --args -NSTextInsertionPointBlinkPeriodOn 1000 -NSTextInsertionPointBlinkPeriodOff 200`. This is the argument domain; it writes no preference.
- Focus it, and expect the log to read `caret: 1000/200 ms`.
- Turn on System Settings' "prefer non-blinking cursor" and come back. Expect `switch on` and no blink frames in the five-second report.
  - This is the only check of the un-observed call's freshness (A8).
  - If it reads `switch off`, the fix is an observer through the window crate.
- Set `blink = true`, and expect it to blink again and the warning to show.

#### 8. Perturbations, one per mechanism

| broken | goes red |
|---|---|
| the unset cycle back to 600/300 | 1, 2, 5, 6 |
| the keys coupled, or Chromium's average (`defaults_utils.mm:72-75`) | 3 |
| NaN taken as AppKit's 250 ms | 3 |
| `<= 0` accepted, clamped to the floor | 3 (not 4: `Phase` keeps it nonzero) |
| no floor | 3, and 4 by a panic at `%` |
| no ceiling | 3, at its `Steady` and ceiling assertions |
| `plan` ignoring the gate | 7, 11 |
| `Gate::of` ignoring the switch | 7 |
| `Gate::of` ignoring an explicit `true` | 7 |
| the stop moved by output | 8 (output arm), 10 |
| the stop never expiring | 8, 10 |
| the stop missing from `next_change` | 8 (next change) |
| a key not reviving a stopped blink | 8 (key) |
| `quiet` a no-op | 8 (click before the stop) |
| `quiet` also moving `epoch` | 8 (click before the stop) |
| the config defaulting to `Blinking` | 14 |
| `"auto"` not parsed | 14, 16 |
| the symbol name misspelt | 17, on macOS 15+ |
| an `RTLD_*` value wrong | 18 |

The drivers' calls are established by reading, as the blink section already does:
- the first-tick and focus-gain reads;
- `send` and `paste` calling `input`;
- `mouse` and `scroll` calling `quiet`;
- the Linux `input` calls.

The gate needs no such reading: it is inside `Caret` (A4).

#### 9. Measurements owed

* **The read in the app.** The logged `read in N us` on the first tick and on a later gain. The probe put the first AX call at 0.9-2.2 ms and later calls under 1 us [run, in a probe, not in the app].
* **`startup-bench.py`, A/A, base against change.** Three stages:
  - `caret os` (new), the read's own cost;
  - `first present`, which is expected flat, since nothing new runs before it;
  - **`first content`** (`BAUD_CMD=shell`, `BAUD_BENCH_STAGE="first content"`), because the first tick at ~180 ms is close to the shell's first bytes at ~193 ms (`CLAUDE.md`'s cold-boot table).
* **Idle, focused, owner-run.** 650/350 against 600/300 should be about 10% fewer blink frames a second.
* **Idle, owner-run.** The switch on against `blink = false`: both should build no blink frames.
* **No ingest sweep.** `baud-core` is untouched.

#### 10. Docs, in the same commit

* `caret.rs`:
  - the module doc: "The timing is xterm's", "There is no timeout", and the 0.9 s figure;
  - the constants at `:38-41`;
  - the `cycle_offset` comment at `:146`;
  - the test docs at `:181-183` and `:195`.
* `app.rs`: `:445-449` ("xterm's times"), and the rewritten comment at `:1476-1482`.
* `linux.rs`: `:17` says "SGR 5 text blink"; `:82-85` gains the stop.
* `config.rs:63-69`.
* `config.toml.example`:
  - the prose at `:63-67`;
  - `:101`, `blink = true` becomes `blink = "auto"`;
  - a new line, `stop_blinking_after = 0`;
  - one sentence saying `true` overrides the system switch.
* `docs/cursor.md:12`.
* `docs/vt-coverage.md:661`.
* `CLAUDE.md`: the blink section at `:18932-18940`, and a new section with the keyframes, the parse table, the gate, the stop and §9's numbers.
* `v-open-decisions.md:59-62`.
* `TODO.md:410-413` and `:421`, locally only; `TODO.md` is gitignored.
* **Sources to pin** before any `[[source:]]` cites them: `AXSettings.h`, and Chromium's `defaults_utils.mm` and `os_settings_provider_mac.mm`. Confirm the Chromium commit at fetch.
* `verify/anim.swift` becomes `scripts/caret-probe.swift`.

### Owner questions
- **Q1, the edges.** The owner's record gives only 650/350 (v-open-decisions.md:52-55); "hard edges" appears only in the workflow brief. AppKit's caret [run, verify/anim-default.txt] ramps into each phase in four 37.5 ms steps.
- Fully opaque 537.5 ms of each 1 s cycle.
- At least half-opaque 687.5 ms.
- The same *mean* opacity as hard edges at 650/350: an integral of 650 ms-equivalent per cycle.

The options:
- **(a) Hard edges at 650/350.** No renderer change.
- **(b) AppKit's ramps.** 8 opacity changes a cycle against 2, so about 4x the blink's frames. Needs a cursor opacity on the CPU, instance and cell paths, a shader change and an oracle scene.
- **(c) Hard edges at the 0.5 crossings**, 687.5/312.5.

RECOMMENDED: (a).
- **Q2, precedence.** RECOMMENDED: the switch steadies the drawn caret unless the config says `blink = true`, with a one-time warning when it does.
- The owner's words support this: "honoured unless the config says otherwise". `false` does not say otherwise, so under `false` and the switch on, an application's `DECSCUSR 1` is drawn steady.
- DECRQSS and DECRQM keep answering the application's style.
- The alternative, the literal "any explicit config ignores the switch", lets an application blink the cursor for a user who wrote `blink = false` and turned the switch on.
- **Q3, the floor and the ceiling.** The parse must be total: a zero phase divides by zero at caret.rs:143-148.
- **Floor, recommended 1/60 s.** Below that, a phase cannot be shown and every tick would owe a frame. It is a cost: a 1/60 s phase at 120 Hz is a frame every second tick.
- **Ceiling, recommended 60 s**, Chromium's `kMaximumReasonableIntervalMs` at defaults_utils.mm:64-71.
  - An on phase past it means steady, as AppKit does at On=inf [run].
  - An off phase past it is clamped to 60 s. AppKit hides the caret forever there [run], and baud refuses a caret that never shows.
- NaN, 0 and negatives fall back per phase. AppKit takes a stored NaN as 250 ms [run]; that case is deliberately not followed.
- **Q4, when the OS is read.** RECOMMENDED: once on the first tick, after the first present, with a startup mark so startup-bench can see the cost, then at every focus gain. No notification observer.
- The switch's freshness without an observer is **not found**. Chromium observes `AXPrefersNonBlinkingTextInsertionIndicatorDidChangeNotification` (os_settings_provider_mac.mm:66-76) and calls the function fresh on each query. It re-reads only the keys on activation.
- The owner-run live check settles it. If it fails, the fix is a notification observer through the window crate, which is a crate change and a release, since app.rs forbids unsafe.
- A change made while baud keeps focus lands at the next gain.
- **Q5, the stop timer.** RECOMMENDED: kitty's semantics. The key is `[cursor] stop_blinking_after = <seconds>`; 0 or absent means never, which is the default, as decided.

It is reset by input only. The input set needs a decision:
- keys through `App::send`, which includes the wheel's arrow keys on the alternate screen (app.rs:3757-3760);
- a paste;
- a focus gain;
- any mouse press, including one reported to a tracking child (app.rs:3701/3743);
- the wheel.

Output never resets it, so `tail -f` stops blinking; kitty's option says "keyboard inactivity".

It stops with the cursor shown.

Before the deadline a click moves only the deadline. After it, any input restarts the cycle shown [inferred from caret.rs:97-106], which is GTK's rule. The alternative is keys-only, which stops the blink for a mouse-only user.
- **Q6, Linux.** RECOMMENDED: keep xterm's pinned 600/300 (`Cycle::XTERM`) and no switch in this build. Say so in the docs, and leave GNOME's `cursor-blink`, `cursor-blink-time` and `cursor-blink-timeout`, read through the Settings portal, to a later build. The alternatives:
- AppKit's 650/350 as the one unset default on both platforms;
- GNOME's 1200 ms cycle now.
- **Q7, `SGR 5` text blink.** It runs on its own 0.4 s half-period (app.rs:77); kitty ties text blink to the cursor's interval (kitty-options-definition.py:428-429). The AX switch speaks of insertion indicators only (AXSettings.h:32). RECOMMENDED: leave text blink's rate and the switch's reach out of this build and decide them on their own.
- **Q8, a rate-override key.** TODO.md:421, a sweep entry and not the owner's record, asks for one; kitty has `cursor_blink_interval`, where -1 means the system (kitty-options-definition.py:414-419). RECOMMENDED: none in this build. The bundled app already takes a per-app override with no code, `defaults write dev.baud.terminal NSTextInsertionPointBlinkPeriodOn -int 400` (Info.plist:7 names the domain). A bare `target/release/baud` reads its own executable domain instead [inferred].
- **Coordination.** `BlinkChoice { Auto, On, Off }` should be the shape the Reduce Motion build reuses for `trail`. `config.toml.example:102` writes `trail = "fire"` explicitly, which is the same trap the example's `blink = true` is here (A17). RECOMMENDED: the same `"auto"` word, the same rule, and the same one-time warning when an explicit value overrides a system setting.

### Risks
- **The first AX call costs 0.9-2.2 ms on the main thread** [run, in a probe, not in the app]. Placed on the first tick, it is after the first present but near the shell's first bytes (first tick ~180 ms, first bytes ~193 ms). So `first content`, not only `first present`, must be in the A/A startup sweep.
- **A failed present can leave the caret hidden with nothing owed.** `caret.commit(caret)` is taken whether or not the present succeeded (app.rs:1476-1482). If the one frame that shows a stopped, switched or steady caret fails, the caret stays hidden until something else draws. Its comment's "at most caret::ON away" becomes false.

The shape predates this change for a focus loss, and this change adds three more ways to reach it.
- **The switch's freshness without an observer is unverified.** Chromium observes the change notification rather than relying on a re-read (os_settings_provider_mac.mm:66-76). Only the owner-run live check can show the focus-gain re-read sees a flip made in System Settings.
- **The dlopen/dlsym path has never run on macOS 13, 14 or 15.0.**
- The version gate and the null-symbol branch are unexercised.
- That AppKit has already loaded Accessibility.framework was run only on macOS 27.
- **A misspelt symbol name reads `switch off` forever with every other test green.** Test 17, the positive control on macOS 15+, is the only thing that can catch it.
- **The live checks are owner-run.** A scripted window is frontmost and not key (CLAUDE.md:19372-19374), so the blink's frames, the switch's effect and both idle measurements need the owner.
- **Configs copied from the old example keep `blink = true`, which now means explicit and ignores the accessibility switch.** The owner's own configs carry no `[cursor]` section [run, grep]. The one-time warning is the only thing that surfaces it for anyone else.
- **The Chromium sources were fetched at the default branch's HEAD**, and the SHA `e5396564` is the report's, not re-confirmed. Pin and confirm before any `[[source:]]` cites them.
- **`doubleForKey` agreeing with AppKit is inferred from matching results** on every string and double case tried [run]. A case where the two diverge would make baud's parse drift from AppKit's.
- **Hard edges (Q1(a)) visibly differ from native text fields' ramped caret.** If the owner picks (b), the renderer needs a cursor opacity on every path and an oracle scene, and the blink's frames grow about 4x.
- **The input set decides who stops the stop timer.**
- Keys-only stops the blink for a mouse user.
- Counting the wheel on the alternate screen (it sends keys through `App::send`) means scrolling `less` holds it off, while scrolling vim under mouse tracking does not unless the wheel also calls `quiet`.

Q5 settles it; any split leaves an asymmetry to document.
- **`set_system` no longer restarts the epoch.** It relies on the `was` edge (caret.rs:98-100) firing at every read. That holds because every read is either on a focus gain, where the blink was `No`, or on the first tick. Any future caller that changes the cycle while the blink stays `Yes` would need the restart back, with a test that drives that path.

## `?12`, the configured blink, and the cursor after vim and helix

This is a read-only brief against `.` at `8fab372`. Nothing in the repository was edited and cargo was not run. Scratch work is under `…/scratchpad/blink12/`: the byte captures are `verify/cap.py` and `verify/cap12.py`, and the fetched tmux and neovim sources are in `ext/`.

Every claim is labelled **run** (captured in a pty here) or **by reading** (from source).

### 0. The premise was wrong, and the correction changes the fix

The task says vim and every curses `endwin` send cnorm (`\E[?12l\E[?25h`) at exit. That is false for vim, and for helix cnorm is only half of what happens.

**vim 9.1** (**run**, `TERM=xterm-256color`, both `-u NONE -N` and the default vimrc):

- At startup it sends `ESC[?1049h … ESC[?12h ESC[?12l … ESC[?25l`.
  - The committed recordings have the same bytes at the same offsets: `tests/recordings/utf8/vim-unicode.raw` at 0/45/51, `tests/panels/vimhl.raw` at 0/37/43, and `tests/panels/vimdiff.raw` at 17/54/60.
- At exit it sends `… ESC[?1049l ESC[?25h`, with **no `?12` at exit**.
  - vim splits cnorm: `t_ve=^[[?25h`, `t_vs=^[[?12h`, `t_VS=^[[?12l` (`vimopts.txt`).
  - vim did not send its `t_RC` DECRQM `?12` query in either run.
- In baud, the startup `?12l` lands on the alternate screen and folds the blink off (`baud-core/src/ops.rs:1024-1026`).
  - The style is one field for both screens (`baud-core/src/term.rs:368`), written only at `term.rs:489/1676`, `ops.rs:1025/1497` and `write.rs:1594-1595`.
  - So the cursor is steady **for the whole vim session and for the shell after it.**

**helix 25.07.1** (**run**, empty config). Exit:

```
… ESC[?25h ESC[2 q | ESC[0 q ESC[2 q ESC[?12l ESC[?25h | ESC[?1006l … ESC[?1049l
```

- The middle group is `reset_cursor_approach` (`docs/research/sources/helix-tui-crossterm.rs:38-51`): `0 q`, then `Se`, then cnorm.
  - It is written before `LeaveAlternateScreen` (`:205-221`), so all of it lands on the alternate screen.
- `Se` is `\E[2 q`, an explicit steady block, from macOS's `/usr/share/terminfo/78/xterm-256color`. `infocmp -x` shows `Se=\E[2 q, Ss=\E[%p1%d q`, and baud gives its child `TERM=xterm-256color` (`baud-app/src/bell.rs:67-74`).
- `TERM=xterm-256color tput cnorm | od -c` gives `033 [ ? 1 2 l 033 [ ? 2 5 h`, and `tput cvvis` gives `033 [ ? 1 2 ; 2 5 h` (**run**).

**Finding: the pinned terminfo lost `Se`.** `docs/research/sources/terminfo/xterm-256color.ti` says "Reconstructed via infocmp" at line 1. It was made without `-x`, so the extended `Se`/`Ss` were dropped. The sentence at `CLAUDE.md:18972` ("the pinned `xterm-256color` entry carries no `Ss` or `Se`, so it cannot say which reset an editor sends") is an artefact of that pin, and the pin should be retaken with `infocmp -x`.

**Why macOS's `Se` is `\E[2 q`, and why baud is the odd one out.**

- `2 q` is xterm's own power-on cursor, a steady block (`DEF_CURSOR_BLINK "false"`, xterm `main.h:126-127`, scratch, unpinned).
- Under xterm's escape-bit model (§1), `2 q` means "escape bit 0: defer to the user's resource".
- So it is a correct reset for xterm and a wrong one for a terminal whose configured cursor blinks.
- baud took xterm's `TERM` without xterm's model. Every other terminal in the table ships its own entry whose `Se` is Ps 0:
  - ghostty `Se=\E[0 q` (`src/terminfo/ghostty.zig:120`);
  - alacritty `Se=\E[0 q` (`extra/alacritty.info:107`);
  - foot `Se=\E[ q` (`foot.info:43`);
  - kitty's cnorm turns the blink *on*: `\E[?12h\E[?25h` (`kitty/terminfo.py:129`).
- Whether Linux ncurses' `xterm-256color` also has `Se=\E[2 q` is **not verified**; there is no ncurses terminfo on this box.

**Two more programs, by reading** (neither is installed here, so neither was run):

- **neovim** (`ext/nvim-tui.c`, neovim `521eddec`, sha256 `fe5f8e0c…`, unpinned):
  - It strips `?12l` off the front of cnorm because, in its own words, DECRST 12 "interferes with the user's control via set_cursor_style" (`:2208-2215`).
  - Its exit sends the stripped cnorm, then `reset_cursor_style` (the terminfo's `Se`, so `2 q` here), then `?1049l` (`:664-666`, `:713`).
  - So it is helix's shape: an explicit steady block, on the alternate screen.
- **tmux** (`ext/tmux-tty.c`, tmux `94796f6b`, sha256 `bdf7e459…`, unpinned):
  - It holds the outer terminal's alternate screen for the whole session.
  - `tty_update_cursor` (`:786`) sends cnorm on every cursor-mode change (`:834`).
  - When a pane returns to the default style, it sends the outer `Se`, so `2 q`, then `cvvis` only if the pane asked for a blink (`:835-845`).
  - So inside tmux, `2 q` arrives on an alternate screen that nothing leaves until detach. Today's `?12l` also means baud stops blinking the moment tmux updates the cursor.

**Consequence: no `?12` rule alone fixes helix or neovim on macOS.** Even with `?12l` ignored, each ends on an explicit `2 q`. What fixes them is either a rule under which `2 q` means "configured" (xterm's XOR), or a `?1049l` that gives the shell back the primary screen's cursor. **tmux is not fixed by either of the reasonable rules below**, because its `2 q` is on a screen that stays up.

### 1. The table

Each terminal's rules are applied, **by reading**, to the **run** vim and helix bytes. The configuration is each terminal's blinking default, or the named setting. "Blinks" or "steady" is the shell's cursor after the editor exits.

| terminal (commit) | `?12l` with blink configured | DECSCUSR 0 | `?1049l` restores shape and blink? | after vim | after helix, macOS `Se=2 q` | after helix, `Se=0 q` or none | DECRQM `?12` answers | source lines |
|---|---|---|---|---|---|---|---|---|
| **baud today** (8fab372) | folds the blink off the single style | configured style (foot's reading) | no: one field, not saved | **steady** | **steady** | **steady** | the drawn blink, so **1 at power-on** | `ops.rs:1024-1026, 1574-1577`; `write.rs:1594-1595`; `dcs.rs:65-67`; `term.rs:368` |
| **xterm** (pinned `xterm-charproc.c`; `cursorBlink: true`, `cursorBlinkXOR` defaults True) | esc=0, 1 XOR 0: **blinks** | esc=1, 1 XOR 1: **steady**, though ctlseqs calls Ps 0 a blinking block | no: `SavedCursor` has neither (`ptyx.h:2347-2363`, scratch); both live on `TScreen` (`ptyx.h:2806, 2811`) | blinks | blinks: `2 q` and `?12l` both leave esc=0 | blinks | the escape bit, so 2 while blinking (`misc.c:5517-5518`, unpinned) | `xterm-charproc.c:147-150, 566-567, 4949-5003` (esc at 4999), `7346-7359, 7626-7630, 8487-8491, 10948-10955, 14382-14383` |
| xterm, default (`cursorBlink: false`) | steady | blinks | no | — | — | — | the escape bit | no configured blink, so the bug cannot arise |
| **kitty** (4136c93; `cursor_blink_interval` -1 blinks) | `non_blinking` set on this screen: steady | configured shape, blinking | **yes, both**: entering 1049 saves then resets the cursor, leaving copies it back, `shape` and `non_blinking` included | blinks | blinks | blinks | **0, not recognised**: no case for mode 12 | `kitty/screen.c:2035, 2039-2041` (1049 passes `save_cursor=true`), `1940-1961, 2706-2720, 3266-3298, 3332-3341`; `cursor.c:247-263`; `child-monitor.c:788-806`; `modes.h:53, 77` |
| **foot** 1.21.0 (pinned) | decset=0; drawn = `decset \|\| deccsusr`: **blinks** | configured style and deccsusr | no: style and blink live on `term` (`terminal.h:594-597`, clone); `term_save_cursor` saves the grid cursor only | blinks | **steady**: `2 q` sets deccsusr=0 | blinks | decset, so 2 while blinking | `foot-csi.c:367, 629, 1753, 1779`; `foot-terminal.c:1313-1317, 2147, 2176, 3001, 3225-3230` |
| **ghostty** (b40acce), `cursor-style-blink = true` | ignored (app layer) | configured | shape per screen, **blink not** (a global mode); `SavedCursor` has neither | blinks | **steady**: `2 q` sets the mode false even under an explicit config | blinks | the mode bit | `src/termio/stream_handler.zig:637-641`; `src/terminal/Terminal.zig:405-424, 4843, 4887`; `Screen.zig:140, 198-206`; `modes.zig:348`; `render.zig:609` |
| ghostty, unset (default, blinks) | mode set false: steady | configured | as above | steady* | steady* | steady* | the mode bit | *masked: shell integration resends DECSCUSR at every prompt (per report, `Config.zig:8840`, zsh integration `:255-266`; not verified by me). kitty's integration does the same per report |
| **alacritty** (d692748), `blinking = On` | writes the terminal style: **steady** | configured (`None`) | no: `cursor_style` is on `Term` (`term/mod.rs:307`), and `swap_alt` (`:714-735`) leaves it | steady | steady | steady | the terminal's bit, not the drawn one | `alacritty_terminal/src/term/mod.rs:1985-1990, 2034-2040, 2052-2056`; `alacritty/src/config/cursor.rs:124-131`; `event.rs:1631`. `Always` blinks everywhere |
| **wezterm** (b09b56c), a blinking default style | **ignored** | configured | **yes**: 1049 does DECSC/DECRC, and the saved position carries the shape, which carries the blink | blinks | blinks: the alternate screen's `2 q` is undone | blinks | **always 2** | `term/src/terminalstate/mod.rs:1553-1563, 2005-2027, 2721-2768` (shape restored at 2762) |

With blink configured, the tallies are:

| case | blinks | steady |
|---|---|---|
| after vim | xterm, kitty, foot, wezterm, ghostty with an explicit config | alacritty `On`; ghostty unset (masked); **baud** |
| after helix, on this machine's terminfo | xterm, kitty, wezterm | foot, ghostty, alacritty, **baud** |

The helix column is kinder to the others than it looks, because each of them sets its own `TERM` (§0).

### 2. Candidate rules

**Cases:**

| name | what the terminal receives |
|---|---|
| vim | startup `?12h ?12l` on the alternate screen; exit `?1049l ?25h` |
| helix/nvim on macOS | `… 2 q ?12l ?25h`, then `?1049l`; nvim's cnorm has no `?12l` |
| helix with `Se=0 q` | `0 q ?12l ?25h`, then `?1049l` |
| tmux | cnorm, then `Se` = `2 q`, on a long-lived alternate screen |
| primary cnorm | `tput cnorm`, or any program that does not use smcup |

**Invariants that tests hold today:**

- DECSCUSR 1-6 draw the blink they name:
  - `decscusr_sets_each_documented_shape` in `baud-core/tests/vt220/decrqm_cursor_style.rs`;
  - `baud-render/tests/cursor_blink.rs:63, 88`.
- `?12h` turns a steady configured cursor into a blinking one:
  - `private_mode_twelve_moves_the_configured_styles_blink`, `baud-core/tests/vt220/decrqm_cursor_style.rs:232-236`;
  - `cursor_blink_does_not_change_the_cursor_shape`, `baud-core/tests/vt100/private_modes.rs:136-154`.
- DECRQSS goldens:
  - `1 q` at the default and `5 q` after `5 q` (`baud-core/tests/golden/fixtures.txt:3347, 3353`);
  - `6 q` under a SteadyBar config after `3 q 0 q`.
- No test sends DECRQM `?12`. A grep for `12$p` finds nothing.

**Two separate questions decide the design:**

1. How does `?12` combine with the configured blink? xterm XORs; ghostty and wezterm ignore it; foot ORs; alacritty, kitty and baud overwrite.
2. Does `?1049l` give the shell back its cursor? kitty and wezterm do. ghostty does for the shape only. xterm, foot, alacritty and baud do not.

#### (A) xterm's XOR: blink = configured XOR one escape bit, shared by `?12` and DECSCUSR

- **Who:** xterm only, and only for users who turn `cursorBlink` on. Its default is false, and there XOR equals the escape bit alone.
- **Cases:** vim, helix/nvim both ways, tmux and primary cnorm all **blink**. It is the only rule that fixes tmux, because `2 q` means esc=0.
- **Steady via `?12l`:** impossible. Under a blinking config, `?12h`, and so cvvis ("very visible"), gives the *steady* cursor.
- **Invariants:** broken. DECSCUSR 1, 3 and 5 go steady and 2, 4 and 6 blink. `CSI 7 SP q` goes steady (`initial_cursor` is the blinking form, `charproc.c:10948-10955`, then esc=1 at `4999`).
- **Truthful answers, xterm's:** DECRQM `?12` and DECRQSS report the escape bit, not the cursor drawn. After cnorm, under a blinking config, xterm answers `?12;2$y` and `2 q` while the cursor blinks.
- **Verdict:** out. It fixes the reported case by inverting every explicit request.

#### (B) Ignore `?12`: when a blink is configured (ghostty), or always (wezterm)

- **Cases:** vim, helix with `Se=0 q` and primary cnorm **blink**. helix/nvim on macOS and tmux are **steady**, because `2 q` is honoured.
- **Steady via `?12l`:** not available. Use DECSCUSR 2, 4 or 6.
- **baud has no "unset" config.** `[cursor] blink` is a bool, default true (`baud-app/src/config.rs:310`), so ghostty's variant collapses into wezterm's unless a third state is added.
- **Invariants:** ignoring it always breaks both `?12h` tests above.
- **Truthful DECRQM `?12`:** the mode is not acted on, so answer DECRPM **3 or 4** ("permanently set/reset") following the configured blink, as xterm reports `?13` (`misc.c:5520-5536`). wezterm's constant 2 is untrue while the cursor blinks. DECRQSS: the style as drawn.

#### (C) Honour `?12`, but treat a cnorm-shaped `?12l ?25h` (or `CSI 0 SP q`) as "restore the configured blink"

- **Who:** nobody.
- baud already restores on `0 q` (`write.rs:1595`). That did not help, because helix's `2 q` and `?12l` come after it.
- **Cases:** matching the byte pair fixes both helix orders and primary cnorm. It **does not fix vim**, whose `?12l` is followed by `?25l` much later. It does not fix neovim, which strips `?12l`, or tmux's `2 q`.
- **Cost:** a heuristic over byte adjacency that needs the read-boundary look-back `Placement` needed.
- **Truthful answers:** as today's fold, DECRQM reports the drawn blink and DECRQSS the fused style. Nothing about the heuristic is visible to a query.
- **Verdict:** out.

#### (D) foot's OR: two inputs, drawn = `decset (?12) || csusr_blink (DECSCUSR, else configured)`

- **Who:** foot.
- **Cases:**
  - vim **blinks**, **inside the session too**;
  - helix with `Se=0 q` blinks, and so does primary cnorm;
  - helix/nvim on macOS and tmux are **steady**, because `2 q` sets `csusr_blink=0`.
- **Steady via `?12l`:** no effect against a configured blink. DECSCUSR 2, 4 or 6 still gives a steady cursor.
- **`2 q` then `?12h`:** blinks, because `decset` wins. That is foot's rule.
- **Invariants:** all kept. The baud map walked every test as passing if `cursor_style()` means the style drawn and DECRQSS answers from `csusr_blink`.
- **Truthful answers, foot's:** DECRQM `?12` reports `decset`, so 2 at power-on while blinking. DECRQSS reports the shape plus `csusr_blink`, which keeps the goldens.

#### (E) The cursor per screen: shape and blink saved at `?1049h`, restored at `?1049l`

- **Who:** kitty (both), wezterm (the shape, which carries the blink), ghostty (the shape only).
- **Cases:**
  - vim **blinks after** exit, but is **steady inside vim**, because vim's own `?12l` holds on the alternate screen;
  - helix and nvim both ways **blink**, since everything they send is on the alternate screen;
  - **primary cnorm is steady for good**, which is today's bug for any program not using smcup;
  - **tmux is steady**, because its `2 q` is on a screen still up.
- **Steady via `?12l`:** works, on that screen.
- **Invariants:** all kept. `?12` still folds as today, per screen.
- **Truthful answers:** DECRQM `?12` reports the live screen's drawn blink, and DECRQSS the live screen's fused style, as today but per screen.

#### (F) D and E together: foot's OR for `?12`, plus the cursor saved and restored across `?1049`

- **Who:** no single terminal ships this combination.
- **Cases:**
  - vim **blinks** inside and after;
  - helix and nvim both ways **blink**;
  - primary cnorm **blinks**;
  - **tmux does not**: a pane's return to the default style reaches baud as `2 q` on tmux's alternate screen, and it stays steady until something sets a blinking style or tmux detaches. Under today's rule it stops blinking even sooner, at tmux's first cnorm.
- **Steady via `?12l`:** no effect against a configured blink. Use DECSCUSR 2, 4 or 6. Under a steady config, `?12h` still turns the blink on.
- **Invariants:** all kept.
- **Truthful answers:** as (D), per screen.
- **Why each half is needed:**
  - **E** is the only rule besides XOR that survives the `Se=\E[2 q` helix and neovim send, and XOR is out.
  - **D** is what keeps a primary-screen cnorm and vim's own session blinking, as xterm (with a configured blink), foot, wezterm and ghostty-explicit do.

#### (G) Not a rule but a complement: stop getting xterm's `Se`

This is the majority's practice. ghostty, alacritty and foot each ship an entry whose `Se` is Ps 0. With it, helix, nvim and **tmux** send `0 q`, which baud already reads as "configured" (`write.rs:1595`). It is the only non-XOR way to close tmux.

Two forms, both owner calls:

- **A baud entry, e.g. `xterm-baud`.** Remote hosts over ssh lack it and need a fallback. ghostty's answer is shell-integration terminfo installation, per the reports, not verified.
- **Shadowing `xterm-256color`** through a `TERMINFO` that baud ships, with `Se=\E[0 q`. This is portable to local programs and changes nothing over ssh. It also redefines a standard name, which no terminal read here does.

(G) does nothing for vim, whose `?12h ?12l` are built in, not `Se`, so it complements (D) rather than replacing it.

### 3. Recommendation: (F) now, with (G) recorded as the owner's call for tmux

This is a recommendation, not a decision.

**Why.** (F) is the smallest rule that gives the shell back the configured blink after vim, helix and neovim on the `TERM` baud actually sets. It never inverts an explicit DECSCUSR. It keeps every existing `?12` and DECSCUSR test and both DECRQSS goldens. Each half follows a terminal that gets its case right: foot for `?12`, kitty and wezterm for `?1049`.

**What it leaves.** Its one residual is an `Se` (`2 q`) sent inside a long-lived alternate-screen host (tmux, by reading), or sent on the primary screen. Only (G), or xterm's XOR, closes that.

**Shape:**

- **Per-screen state:** shape, `csusr_blink`, `decset`.
  - `Term::cursor_style()` stays the drawn style on the live screen: `shape + (decset || csusr_blink)`. `baud-app/src/caret.rs:54-60`, `baud-render/src/renderer.rs:921, 1363`, `baud-app/src/app.rs:1180, 3853` and `baud-app/src/linux.rs:305` then do not change.
  - The two-state fields are enums, since `baud-domain` is gated at zero `bool` fields.
- **`?1049h` saves the primary's cursor and `?1049l` restores it.**
- **`RIS` and `DECSTR` clear `decset`** and restore the configured shape and blink. xterm does this on both resets (`charproc.c:14382-14383`), foot on RIS only (`foot-terminal.c:2176`; `2147` returns early for a soft reset).
- **DECRQM `?12` answers the `decset` bit**, as xterm, foot and alacritty's terminal bit do.
  - It round-trips: an application that saves the mode with DECRQM and restores it with `?12h/l` gets its own state back.
  - Answering the drawn blink, as today, would make that application set `decset=1` from a configured blink, which then overrides its own later `2 q`.
  - Cost: the power-on answer becomes 2 while the cursor blinks. Nothing pins today's 1.
  - Optional: a read-only `?13` reporting the configured blink as 1 or 2, as xterm does (`misc.c:5520-5536`), so an application can learn the user's preference honestly.
- **DECRQSS `SP q` answers the shape plus `csusr_blink`**, as foot does (per report, `dcs.c:420-437` of foot 1.21.0, a scratch fetch not pinned). The goldens are unchanged.
- **Housekeeping:**
  - re-pin `xterm-256color` with `infocmp -x`;
  - correct `CLAUDE.md:18972-18975`, and `docs/vt-coverage.md:579` (the `?12` row "done") and `:646` (the `cursorBlinkXOR` sentence);
  - rename any renamed test at `CLAUDE.md:18988-18998` too, or `docs.rs` fails.

**The gates it needs**, each red first:

- **Recorded:** a fixture of helix's and vim's captured start and exit bytes, reproducible with `verify/cap.py` and committed with the command. Each must end on the configured blink.
- **Unit:**
  - a `?1049` round trip across `2 q` and `?12l`;
  - `tput cnorm` on the primary screen;
  - DECRQM `?12` and DECRQSS after `?12` and after DECSCUSR;
  - `2 q ?12h` blinks;
  - a tmux-shaped `2 q` on a screen that stays up, pinned as steady so (G) can move it.
- **Perturbations:** one per mechanism: the OR, the save, the restore, each reset.

**If the owner wants the minimum:** (E) alone fixes the three editors and follows kitty and wezterm. It leaves the cursor steady inside vim, after any cnorm on the primary screen, and inside tmux.

#### Owner calls to record

1. Where the cursor is saved.
   - **In `Saved`:** plain DECSC/DECRC restore it too. kitty and wezterm do this; xterm, foot, ghostty and alacritty do not.
   - **In `?1049`'s save only.**
2. Whether `decset` is per screen or global.
3. What `?1049h` hands the alternate screen:
   - **the primary's cursor**, as xterm and wezterm keep it and ghostty copies the shape;
   - **the configured default**, as kitty does (`cursor_reset`, `screen.c:1955`).
4. `?47`. baud's bare switch shares one cursor (`Term::primary_cursor` gives `Shared`). Should the style be shared too, with no save? kitty resets it on entry and does not restore.
5. Whether `CSI 0/7 SP q` clears `decset`.
   - foot's `0 q` leaves it (`foot-csi.c:1753`).
   - xterm's shared bit is overwritten.
6. DECRQM `?12`: the `decset` bit (recommended) or the drawn blink. And whether `?13` and `?14` get read-only answers rather than "not recognised".
7. (G): a baud terminfo entry, a shadowed `xterm-256color`, or neither. This decides whether tmux keeps the blink.

## Ride-scroll

Labels: **[R]** read at source, **[run]** run here, **[inf]** inferred. Line numbers are baud at `1ce14c8`. Nothing in the repository was edited and no cargo command was run there.
- A durable copy of this review is at `$TMPDIR/scratch/cursorq/rv2/ride-scroll-design-reviewed.md`.
- The streaming simulation is `ride_sim.py`, beside it.

### A. What the review found

The (a)+(b) reading survives, and so do the fire's noise anchor (`rode` in `k[3]`), the rest state that never moves, and the gate set. Six things do not, and three of them block the commit.

**1. BLOCKING: a ridden trail never lands while output streams.**
- `ride` rebases the moving segment at `now` with its own constants. That is right for one retire. Under streaming, though, every whole-screen retire adds another `cell_h` to each corner's gap, and the gap only decays in between.
- Simulated [run], for a trail in flight when a stream starts, with a 34 px row each retire:

  | corner | a line every 16.7 ms | every 30 ms | every 60 ms | after the last ride |
  |---|---|---|---|---|
  | quad, slow corner (0.4 s) | 3.0-4.0 rows above the cursor | 1.5-2.5 | 0.6-1.6 | 323 ms more |
  | quad, fast corner (0.1 s) | 0.5-1.5 | 0.1-1.1 | 0.0-1.0 | 66 ms more |
  | sweep (ln 2) | 5.5-6.5 | 2.9-3.9 | 1.2-2.2 | 609 ms more |

- No corner ever gets within 0.5 px, so `lasts()` never runs out, and the corners never all leave the window, so the design's drop rule never fires [run+inf].
- So a quad or sweep streak hangs above the cursor for the whole build log. `owes()` asks for a frame every tick for all of it (`baud-app/src/trail.rs:302-313`) [R].
- At a line every 100 ms the slide forces frames for only 60 ms of each 100 (`SLIDE_SECS_PER_ROW`, `baud-render/src/scroll.rs:29`) [R]. The trail therefore adds frames the slide would not draw [inf].
- This contradicts the driver's own rule, stated at `baud-app/src/trail.rs:16-23` [R]: kitty's rule waits for streaming output to stop rather than chasing it.
- Effects are bounded by their own life, because `frame` returns `None` once the ignition is out (`baud-app/src/trail.rs:182-194`) [R].
- Fix (§B.2): fold the segment in once its rides exceed `THRESHOLD` rows.

**2. BLOCKING: the fire's shader change needs `baud.metallib` rebuilt.**
- `fire_flame` is compiled into the committed library. `the_library_was_built_from_the_shaders_beside_it` (`baud-present-macos/tests/metallib.rs:28`) checks its FNV digests [R].
- `scripts/build-metallib.sh` has to regenerate `baud.metallib`, `baud.metallib.fnv` and `shaders.metal.fnv`. It needs the Metal toolchain through `xcodebuild` (CLAUDE.md, "Row 1 shipped") [R].
- Every shader perturbation needs its own rebuild.
- The byte-identity check has to compare against HEAD's shaders rebuilt with the same toolchain [inf].

**3. BLOCKING: `CellPath` does not exist, and the two slide gates can disagree.**
- `grep` finds no `CellPath` [R].
- A `bool` in its place counts against `baud-app`'s signature floor of 4 (`impl_audit.py:15`, `audit_all.sh:233-242`) [R].
- The two gates:
  - `pump` gates the retire on `cell.is_some()` (`app.rs:3453-3458`) [R].
  - `present` gates the pan on `(Some(Mode::Gpu), Some(cell))` (`app.rs:1201-1206`) [R].
- They can disagree. `cell` is built once (`app.rs:737`) and never dropped, while a resize can move the presenter to the CPU (`app.rs:1114-1119`) [R].
- `Slide::retire` returns early when `cell_h <= 0` (`scroll.rs:66-71`) [R], leaving (b) with no pan to pair with [inf].
- Fix: one predicate in front of the ride and the lift, and a ride only when the slide moved. Leave the slide's own gate as it is: on a path with no slide, (b) alone is already rigid with the text [inf].

**4. The drop threshold misses the leaving row.**
- The leaving row is drawn at `[pan-h, pan)`, so it is visible in `[0, pan)` (`shaders.metal:1063`, `:1153`) [R].
- Dropping at `y < 0` therefore removes a trail that is still on screen.
- Drop only when every corner is at `y <= -cell_h - 0.5`. The quad covers half a pixel past its corners (`quad_cover`, `(0.5 - signed)`) [R].
- The ghost is a box `h` tall, so drop it only at `top <= -2*cell_h` [inf].

**5. `ride_ghost` does not move the ghost with the text.**
- The ghost is a linear lerp (`app.rs:80`, `:1013-1030`; `baud-render/src/trail.rs:66-91`) [R]. Moving `from` up `n` rows moves the drawn ghost only `(1-p)*n` rows [inf].
- `Ghost::top` is an unsigned `Px`, clamped at 0 (`trail.rs:57-59`) [R].

**6. The ignition's head rides with the text, which the design's risk text denies.**
- `Ignition::translated` moves `tail` and keeps `dir`/`len`, so the head `tail + dir*len` rides too [inf; `effect.rs:163-205`].
- Parts drawn from the path move with the text [R]:
  - the lightning's bolt and its sparks (`lightning.rs:82-97`, `:314-346`);
  - the ice's frost (`shaders.metal:505-550`);
  - the fire's embers and burn (`fire.rs:109`, `shaders.metal:233-234`).
- Parts drawn from the hole or the emitter stay with the cursor:
  - the lightning's corona (`lightning.rs:290`, `:301`) [R];
  - the plasma's braid (`shaders.metal:289`) [R];
  - the liquid's drop (`liquid.rs:149-152`) [R];
  - the orb's aim.
- So a ridden bolt ends `n` rows above its own corona [inf].
- At the cursor the burn becomes `u = 1 + n*h*dir.y/len`. That dims a flame that moved up [inf].
- Why translate rather than stretch: stretching changes `len`, which re-lays each effect's geometry:
  - `Levels::of(len)` (`lightning.rs:126`);
  - `shed()`'s count (`fire.rs:59`);
  - the ice's lattice along `lu = len/c` (`shaders.metal:505-520`) [R].

#### Facts restated

- **Owner question 3's rationale is overstated.**
  - At HEAD, Enter after a command longer than two cells is already a long move by `near()` (`trail.rs:343-347`) [R+inf]. So the default fire already lights on nearly every command.
  - That trail is drawn along the new prompt's row, which the command left when the LF scrolled it up [inf].
  - Moving the rest state would change three things instead: one- and two-letter commands would trail, the origin would become the command's line, and after long output that origin would be off the window.
- **The cursor-covering breach is narrower than claimed.**
  - With the cursor on the last row, the part of the drawn cursor the unlifted hole misses is `[fh, fh+pan)`, which is clipped (`shaders.metal:1153`, `:1224`) [R+inf].
  - The breach shows only when a whole-screen retire leaves the cursor above the last row: `SU`, or output followed by a cursor-up in the same read.
  - Test 14's mid-screen scene is still the right gate.
- **Citations.**
  - The TODO entry is `TODO.md:410`, not 388 (`TODO.md:421`) [R].
  - `THRESHOLD` is at `baud-app/src/trail.rs:59` [R].
  - `docs/cursor.html` has no line about scrolling to edit [R].
  - `Smear::bounds` has no upper clip: negatives clamp to 0 through `round_px`. The effect bands do clip to `[0, fh)` (`effect.rs:256-257`) [R].
- **The sweep lasts longer.** Its `lasts()` at the floor is 0.813 s; the design's 0.469 s is the quad's slow constant [run].
- **The decision.** The repository records "The trail rides smooth scroll by default, with an option to stay put" (`v-open-decisions.md:51`) [R]. It does not name Neovide; that attribution is the brief's.
- **Neovide's rule is nightly-only.**
  - `b56abb6` is contained only in `nightly` [run].
  - The clone's `mod.rs` hashes to the pinned `neovide-cursor-renderer` [run].
  - 0.16.2 (`d2d6ebd`) has no corner shift [R].
- **Checked and holding [R]:**
  - the reading table;
  - the fire is the only effect that samples absolute-pixel noise (`shaders.metal:225-227`; the ice's rime uses `rel`, `:534`);
  - no effect's `end` reads the tail;
  - the ufo's hover side is absolute (`ufo.rs:189`);
  - `the_uniform_is_effect_us_layout` compares `u[24..32]` with `vfx.params()` (`effect.rs:615-637`);
  - `relayout` snaps the chase (`app.rs:4794`);
  - a switch snaps the chase and the slide (`app.rs:3464-3477`);
  - `linux.rs` has no trail or slide (`grep` is empty);
  - the audit does not count `f32` (`impl_audit.py:3`).

### B. The corrected design

#### 0. Decision

- The owner, 2026-09-25: "The trail rides smooth scroll by default, with an option to stay put" [R].
- "Neovide's choice" is the pinned nightly rule, lines 330-365: a corner moves by its text's drawn motion [R].

#### 1. Reading

The design's table stands [inf, checked]:
- the text's drawn change at a retire is `-n*h + Δpan`, and the cursor's is `+Δpan`;
- (a)+(b) is rigid with the text for path-anchored parts, and with the drawn cursor for cursor-anchored parts.

Which parts are which:
- **Path-anchored:** a moving segment's corners, the whole ignition path (its head and the fire's noise included), and the ghost's origin.
- **Cursor-anchored:** the segment's `to`, `target`, `pending`, the hole, and a landed emitter.

Two additions:
- **The ride is bounded.** A moving segment rides at most `THRESHOLD` rows.
  - On the retire that takes it past the bound, the ride is applied, so that frame stays rigid.
  - The segment then folds in on the fast constant (`Segment::quick`, `baud-render/src/trail.rs:247-255`) and rides no more.
  - This is the driver's existing fold-in for a short move in flight [R], applied to scrolling; it converges on kitty's wait-for-stop [inf].
  - The ignition path keeps riding, because its life bounds it.
- **The rest state never moves** (owner question 3).

Both additions depart from Neovide's rule, which never stops riding and moves the rest state too (`previous_destination += scroll_delta`, 359-362) [R]. Owner questions 3 and 6 put the unbounded numbers beside the defaults.

#### 2. State, and where each piece lives

**`baud-render` (`effect.rs`, `fire.rs`, `shaders.metal`)**
- `Ignition` gains `rode: f32`. `ignite` sets it to 0.0 and `at` keeps it.
- `Ignition::translated(dy)` moves `tail.y` and `rode`, and leaves `dir`, `len`, `seed`, `unit` and `age`.
- `fire::params(&Ignition)` returns `[HEIGHT, RISE, DRIFT, rode, 0, 0, 0, 0]` from the Fire arm (`effect.rs:367`).
- Shader and `fire_reference` both use `q = (p - (0, k[3])) / c`. Then run `scripts/build-metallib.sh`, which regenerates the library and both digests.
- `Vfx::translated(dy)` moves the corners, the hole and the ignition. The axis does not move.

**`baud-render/src/trail.rs`**
- `Smear::translated(dy)`.
- `Trail` gains `lift: Px` and `origin_dy: f32`, with no `Default`.
- `ghost()` computes `top = lerp(row_px(from) + origin_dy, row_px(to), p) + lift`.
- `TrailLook::lifted(pan)` is the (a) half: a ghost sets `lift`; a smear or an effect is translated by `+pan`.

**`baud-app/src/trail.rs`**
- `Chase` gains `ride: Carry { Riding(f32), Folded }`. `take` and `snap` reset it to `Riding(0.0)`.
- `Chase::ride(dy, now, floor, cell_h)` does four things:
  1. **A segment in flight whose carry is `Riding(r)`.** Let `from'` be `seg.at(t)`, each corner moved by `dy` and clamped at `>= floor`. Then:
     - if every corner of `from'` is at `y <= -cell_h - 0.5`, set `moving = None`;
     - else if `|r + dy| > THRESHOLD * cell_h`, set `moving = Some((Segment::quick(from', seg.to), now))` and the carry to `Folded`;
     - else set `moving = Some((Segment { from: from', ..seg }, now))` and the carry to `Riding(r + dy)`.
  2. **A `Folded` segment** is not moved.
  3. **A burning ignition** is translated by `dy`, clamped at the floor, and `rode` takes the amount applied. `at`, `end`, `seed` and `last_lit` are unchanged.
  4. **Untouched:** `target`, `pending`, `axis`, `next_seed`, `last_lit`.
- The clock is `HostTime::of_secs(tick_target)`, the same one `frame` reads (`app.rs:1058-1059`) [R].
- The floor is `-(rows*cell_h)`.
- Rides at one instant compose: the clamp composes for negative `dy`, and the drop is monotone [inf].

**`baud-app/src/app.rs`**
- `enum SlideDrawn { Cells, Other }` is built from the presenter's mode and the cell memory.
- `fn slides(ScrollMode, window: Rows, Screen, SlideDrawn) -> bool` is free and pure. It is true iff the mode is Smooth, `window` is 0, the screen is Normal and the draw is Cells.
- **`pump`** keeps its existing retire gate, including `&& self.app.back.index() == 0`.
  - `every_reader_of_the_window_asks_the_clamp` asserts `raw == allowed` exactly (`app.rs:2627-2630`) [R].
  - After `self.app.slide.retire(n)`, it asks `slides(scroll, looking(&self.app.term, self.app.back), screen, drawn)`. A line containing `looking(` is filtered out of that gate (`app.rs:2567-2575`, `:2622`) [R].
  - If `slides` is true, the pan is above 0 and the key is `Ride`, it calls `self.app.chase.ride(-(n*cell_h), tick_target, -(rows*cell_h), cell_h)` and `self.app.trail = self.app.trail.and_then(|st| ride_ghost(st, dy, p, cell_h))`.
- **`present`** computes `lift` once, before `set_trail`, from `slides(..looking(&term, back)..)` with `pan > 0`. The lift feeds four things:
  - `Scene::pan` (1304);
  - `CellInput::slide` (1388);
  - the leaving-row ensure and copy (1229-1283);
  - `look.map(|l| l.lifted(trail_lift(self.trail_scroll, lift)))`.
- `fn trail_lift(TrailScroll, Px) -> Px` returns the pan under `Ride` and 0 under `Stay`.
- `fn ride_ghost(TrailState, dy, p, cell_h) -> Option<TrailState>`:
  - adds `dy/(1-p)` to `origin_dy`, with `p` taken at `tick_target`;
  - returns `None` when `p >= 1`, because `tick_target` advances before `pump` (`app.rs:3830`, `:3910`) [R];
  - returns `None` when the ridden top is at or above `-2*cell_h`.

  `TrailState` gains `origin_dy`. A ghost partly above the window is drawn clamped at `y = 0`, because `Px` is unsigned; that is stated rather than fixed, for its 0.1 s.

**`baud-app/src/config.rs`, `config.toml.example`**
- `[cursor] trail_scroll = "ride" | "stay"` becomes `enum TrailScroll`.
  - It is read quoted or bare.
  - Any other value gives a complaint and `ride`, as `[scroll] mode` does (`config.rs:458-470`) [R].
- The key goes after `trail = "fire"` (`config.toml.example:99-102`).
- Its comment says three things:
  - "stay" is the pre-build behaviour;
  - under it a trail may lie over part of a sliding cursor above the last row for up to 60 ms;
  - the key is inert under `normal`.

#### 3. What the renderer is handed

- `set_trail(Option<TrailLook>)` is unchanged. The look arrives in drawn pixels, and the bands' `[0, fh)` clip is unchanged.
- No change to `metal.rs` or `glyph_vertex`. The one shader edit is the fire's noise, and it needs the library rebuilt.
- Under `Stay`, at rest, and on every path but the cell path, the look is HEAD's.

#### 4. Idle behaviour

- An idle screen retires nothing, so its lift is 0, and `lifted(0)` is the identity [inf].
- **Before the bound,** a rebase can lengthen `lasts` by up to 0.32 s (the quad's slow corner) or 0.61 s (the sweep) after the last ride [run].
- **Past the bound,** a folded segment lands in about 0.08-0.12 s [inf]. A screenful drops it.
- Effects are bounded by their life.

#### 5. Tests, red first at every level

**Unit, `baud-render`**
1. a_translated_look_moves_every_piece_by_the_same_amount, which includes the ghost's `lift`. Red on identity stubs.
2. a_fire_that_never_rode_reads_no_offset: a guard, fired by a perturbation.

**Models**
3. every_effect_rides_rigidly: `model(v.translated(d), p + (0, d)) == model(v, p)`, and every particle moves by `(0, d)`.
   - Red on HEAD's fire.
   - The ufo within 1.35 cells of the top is the pinned exception.
   - The other six effects are guards and pass from the start.

**Unit, `baud-app`**
4. a_ride_rebases_the_segment_and_keeps_its_constants, for `|dy|` up to `THRESHOLD` rows.
5. a_ridden_trail_lands_on_the_cursor.
6. a_landed_trail_rides_nothing_and_scroll_is_no_move.
7. two_rides_at_one_instant_are_one_ride_of_their_sum.
8. a_ride_stops_one_window_above_the_top.
   - **8b.** Dropped at `y <= -cell_h - 0.5` and kept inside `(-cell_h - 0.5, 0)`. The second edge is new.
   - **8c.** The ghost: a rigid drawn change, `p >= 1` gives `None`, kept at a top inside `(-2h, 0)`, dropped at `<= -2h`.
   - **8d (new).** a_ridden_trail_lands_while_output_streams: a quad and a sweep in flight, `ride(-h)` every 16.7 ms for 2 s with a `frame` each tick. Each must land within 0.2 s of its third ride, and the chase must stop owing. Red on the design as written [run].
9. a_fire_rides_its_path_and_keeps_its_life.
10. the_drawn_trail_moves_as_the_text_does_across_a_retire.
    - (i) at the instant, for `n` in {1, 2, 5}, from settled and from mid-slide. It now also covers the ghost's drawn top.
    - (ii) over the slide: the fire's tail and, before the bound, the quad's corners.
    - HEAD passes only (i) with `n = 1` from settled; (a) alone passes only (ii).
11. The truth table of `slides` and `trail_lift`.
12. The config reader and `the_committed_example_parses_to_what_it_documents`.
13. Two source gates: `self.slide.pan()` is read once, and `.chase.ride(` appears once, after `.slide.retire(n);`, on `tick_target`.

**Pixel oracle, `cells.rs`**
14. The drawn cursor is bare mid-slide, with the cursor mid-screen. Red on HEAD.
15. A ridden effect matches its reference.
    - The reference reads `k[3]` first (red), then the shader changes and the library is rebuilt (green).
    - The other six effects are guards.
16. Byte identity at rest and under `Stay`, against HEAD's shaders rebuilt with the same toolchain. Recorded, not committed.

**No recorded level:** the corpus is core bytes, and this build changes none.

#### 6. Perturbations

The design's table stands, and each shader row needs a rebuild. Four rows are added:

| broken | red |
|---|---|
| drop at `y < 0` | 8b |
| no bound | 8d |
| folds but keeps riding | 8d |
| ghost `origin_dy` without `/(1-p)` | 8c, 10 |

#### 7. Measurements owed

1. **`idle-cpu.py`.**
   - Arms: HEAD, ride, stay, and an A/A copy of ride.
   - Protocol: on mains, unfocused, five rounds of 10 s, first dropped.
   - Movers:
     - idle;
     - a jump, then three lines 50 ms apart, every 400 ms;
     - a line every 16 ms with a jump every 400 ms;
     - **new:** a line every 100 ms after a jump, the case where the slide is idle between lines.
   - Report frames per 10 s and CPU, with ranges.
2. **Clips** of the second and third movers under ride and under stay: the streak and the fold.
3. **No ingest sweep.**

#### 8. Docs

- **CLAUDE.md:** a new section with the table, the bound, the noise anchor and the metallib rebuild.
- **`docs/cursor.md:236-237`:** replace the line.
- **`docs/cursor.html`:** it has no scrolling line to edit. Its lede at :221, "Liquid is wanted and not built yet", is stale and out of scope.
- **`config.toml.example`:** the key.
- **`v-open-decisions.md`:** "Built since".
- **`TODO.md:410`:** update locally, never committed.
- **Drive-by:** `baud-render/src/scroll.rs:2-3` says "Off by default", while `config.rs:284-288` defaults to Smooth [R].

### Owner questions
- 1. The key. Recommended: `[cursor] trail_scroll = "ride" | "stay"`, default ride, as `enum TrailScroll`, read quoted or bare; any other value gives a complaint and falls back to ride. Alternative: `[scroll] trail`.
- 2. What 'stay' means. Recommended: exactly HEAD. Nothing moves and the look is handed through unchanged.
- The cost, scoped correctly: a trail's light can cover part of the sliding cursor for up to 60 ms, and only when a whole-screen retire leaves the cursor above the last row, as after SU or a cursor-up following output in the same read. With the cursor on the last row, the uncovered strip `[fh, fh+pan)` is clipped (shaders.metal:1153, :1224).
- Alternative: move only the hole. That uncovers a cursor-sized block of flame over the line above for up to 60 ms (shaders.metal:230-231; trail.rs:201).
- 3. The rest state (target/pending). Recommended default: it never moves, so the set of moves that trail stays exactly HEAD's, and the ride only transforms trails already in flight. The corrected facts:
- At HEAD, Enter after any command longer than two cells is already a long move (trail.rs:343-347), so the default fire already lights on nearly every command.
- That HEAD trail runs along the new prompt's row, starting from a cell the command has left, because the LF scrolled the command up.
- Neovide's literal rule (`previous_destination += scroll_delta`, mod.rs:359-362) would start the trail from the command's own line, which is text-correct.
- The literal rule would also make one- and two-letter commands trail, and after long output the origin would be off the window. It would need a rule to forget a target that ends wholly above the leaving row.
- 4. The fire's noise anchor. Recommended: the accumulated offset `Ignition.rode` in the free `k[3]`, so a fire that never rode is byte-identical to HEAD. It needs `scripts/build-metallib.sh`.
- Alternative: anchor the noise to the tail. That is simpler, but it changes the fire's look at rest once, and the committed clip becomes a previous build's.
- 5. Riding a plain scroll. Recommended: no ride under `[scroll] mode = "normal"`, on the CPU path, or on the alternate screen, because the owner said 'rides smooth scroll'.
- Note: on a path with no slide, (b) alone is already rigid with the text. Riding a plain scroll would be a gate widening, not new geometry. Neovide moves its corners with the text even with scroll animation off.
- 6. The streaming bound. This is new, and it departs from Neovide, which never stops riding. Recommended: a segment rides at most THRESHOLD (2) rows and then folds in on the fast constant (`Segment::quick`), riding no more. That is the driver's own fold-in rule, and it converges on kitty's rule of waiting for streaming to stop.
- Unbounded (Neovide's rule), measured at a line every 16.7 ms: the quad's slow corners sit 3.0-4.0 rows and the sweep 5.5-6.5 rows above the cursor, never land while output streams, and owe a frame every tick. After the stream they last 323 ms and 609 ms more (ride_sim.py, run).
- Other alternative: drop the segment at the bound. That makes the trail pop.
- 7. A start ridden above the leaving row. Recommended: drop the moving segment once every corner is at `y <= -cell_h - 0.5`.
- For a sweep this makes a visible band pop, because the sweep draws from the tail to the hole even when its tail is off the window.
- Alternative for the sweep only: fold it in.
- 8. The ignition's head rides. Recommended: translate the whole path, tail and head, keeping dir and len, because stretching re-lays geometry: `Levels::of(len)` re-rolls the lightning, `shed()` changes the ember count, and the ice's lattice is laid along `len`.
- The consequence: after a ride, the path-drawn parts end n rows above the cursor while the hole-drawn parts stay with it. Path-drawn: the lightning's bolt and sparks, the ice's frost, the fire's burn and embers. Hole-drawn: the lightning's corona, the plasma's braid, the liquid's drop, the orb's aim. For example, a ridden bolt ends above its corona.
- Alternative: stretch the path to keep its head on the cursor, and accept the discontinuities.
- 9. The ghost. Recommended: ride it exactly. Its origin moves by `dy/(1-p)` in f32 pixels, so the drawn ghost moves with the text. It is dropped when `p >= 1` or its top is at or above `-2*cell_h`, and clamped at y = 0 when partly above the window.
- Alternative: exempt it, since it is 0.1 s and not the default. Test 10 would then pin the exemption.
- 10. The constants after a ride, below the bound. Recommended: a memoryless rebase with each corner's own constant. Alternative: re-pick the constants with `Segment::new`, as kitty and Neovide do on a retarget.
- 11. Which Neovide. Recommended: the pinned nightly rule (b56abb6, contained only in `nightly`). Alternative: release 0.16.2's destination-follows-scroll behaviour, with no corner shift (d2d6ebd, mod.rs:300-330).
- 12. Smaller cases:
- A region retire does not ride; the slide snaps there (app.rs:3451-3452).
- `Ring::Restored` does nothing.
- A read that `moved_since` misreads as Restored, because it retired more than half the ring, does not ride.
- The ufo's hover side reads the hole's absolute top (ufo.rs:189). A lifted hole on row 1 can flip it mid-slide. Recommended: accept and pin it.

### Risks
- The fire's shader changes, so `scripts/build-metallib.sh` has to rebuild `baud.metallib` and both FNV digests with the Metal toolchain, or `the_library_was_built_from_the_shaders_beside_it` goes red. Every shader perturbation needs its own rebuild. The byte-identity check has to compare against HEAD's shaders rebuilt with the same toolchain; toolchain drift could otherwise change unrelated effects.
- Without the streaming bound, a ridden quad or sweep never lands while output streams. At a line every 16.7 ms it sits 3-4 rows above the cursor (sweep 5.5-6.5), owes a frame every tick, and adds frames where the slide is idle between lines (run: ride_sim.py). The bound fixes this and departs from Neovide's rule; the owner may prefer the unbounded rule.
- The ignition's head rides with the text while the hole and the emitter stay with the cursor. A ridden lightning bolt ends n rows above its corona and sparks. Ice frost stops above the cursor. The fire's burn at the cursor dims by `n*h*dir.y/len` for a move up.
- The rest state never moving keeps HEAD's pre-existing text-wrong Enter trail: it runs along the new prompt's row from a cell the command has left, rather than from the command's line.
- Pump's slide gate (`cell.is_some()`) and present's (`Mode::Gpu`) can disagree after a CPU fallback at resize. That mismatch is pre-existing: the slide forces frames and draws nothing. This design leaves it alone and gates only the ride and the lift on the shared predicate.
- Dropping a sweep whose tail rides above the leaving row makes a band that is visible on screen pop.
- A ghost partly above the window is drawn clamped at y = 0 for up to 0.1 s, because `Ghost::top` is an unsigned `Px`.
- The ufo's hover side reads the hole's absolute top (ufo.rs:189, shaders.metal:595), so a lift on row 1 can flip it mid-slide. It is pinned as the equivariance test's exception.
- `dir`, `len`, `tau` and `Vfx::axis` are frozen at ignition and at segment start. After a ride, the re-aim toward the cursor runs on constants and an axis picked for the original move.
- `tick_target` can be up to one tick stale on a wake. `ride` and `frame` read the same value, so the trail stays continuous, but its absolute time lags by up to one tick.
- No test drives `present` or `pump`. The wiring is established by reading plus the two source gates, which is the house convention and weaker than a behavioural test.
- The equivariance and oracle tests live in the macOS test crate, where the fire and plasma references are, so the Linux cross-lint never runs them.
- Adding `rode` to `Ignition` and `lift`/`origin_dy` to `Trail` touches every construction site, including the oracle harness and the existing `PartialEq` comparisons.
- Neovide's rule is nightly-only (b56abb6). Only `cursor_renderer/mod.rs` is pinned, so `rendered_window.rs` must be pinned before any doc cites it through `[[source:]]`.
