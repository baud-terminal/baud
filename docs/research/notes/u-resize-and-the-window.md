# U. Who holds the window over the history across a resize

The owner asked whether `Term::resize` is the right home for reflow, and this
note is the answer and what it rests on. **The re-cut stays inside resize,
eagerly. What is out of place is the scroll position**, which the app holds
as an integer and hands to `Term::resize_holding` so the core can say where
it went. Six shipping terminals were surveyed on three questions: how each
re-cuts, how often, and who holds the viewport across the re-cut. A second
reader re-checked each survey before this note used it.

## Provenance

- **baud** was read at `1413048`, and every baud line number below is that
  commit's. Two later commits act on this note, which is why some items
  below are marked done:
  - `27afdd9` fixed D1 and D2, and made `altresize.rs` able to fail. It
    also found and fixed that a growth at a prompt raised no mark on the
    *primary* either: nothing but the gated re-cut touched the primary's
    marks. Making the marks
    honest exposed that the height step's content test reads a mark, which
    is the owner's question 9 below. The record is CLAUDE.md, "A width growth
    keeps the marks honest at a prompt".
  - `a10b440` corrected the four stale passages this read found, which
    were the draft's appendix and are gone for that reason, but missed one
    of the three places the second of them named: `Term::screen`'s doc,
    still calling the primary parked. The commit that corrects this note
    fixed it, with the alternate screen's surface doc in `Storage`, which
    sat above `clusters` rather than `alt`.
- **Prior art.** A claim that stands on a verifier's correction is marked
  **[corr]**. A claim that stands on reading alone, never run, is marked
  **[read]**. Every file and issue the tables cite is pinned in
  `docs/research/sources/MANIFEST.toml`, re-hashed against the copy the
  survey read: forty entries as group U, and `alacritty-term-term-mod`
  reused from group J, whose copy is byte-identical at `94e7c88`:

| implementation | pin | sources |
|---|---|---|
| alacritty | alacritty_terminal 0.26.0 (vcs `94e7c88`) | [[source: alacritty-grid-resize-0-26]] `grid/resize.rs`, [[source: alacritty-grid-mod-0-26]] `grid/mod.rs`, [[source: alacritty-term-term-mod]] `term/mod.rs` (the same file at `94e7c88`), [[source: alacritty-issue-2408]], [[source: alacritty-issue-4424]], [[source: alacritty-issue-4966]] |
| foot | 1.21.0 | [[source: foot-grid-c-1-21]] `grid.c`, [[source: foot-render-1-21]] `render.c`, [[source: foot-terminal-h-1-21]] `terminal.h`, [[source: foot-issue-939]], [[source: foot-issue-2011]], [[source: foot-c8470f40]] |
| ghostty | `9bbb9b2` | [[source: ghostty-pagelist-9bbb9b2]] `PageList.zig`, [[source: ghostty-terminal-9bbb9b2]] `Terminal.zig`, [[source: ghostty-termio-thread-9bbb9b2]] `termio/Thread.zig`, [[source: ghostty-termio-9bbb9b2]] `Termio.zig` |
| kitty | v0.49.0 (`07ed6aa`) | [[source: kitty-screen-0-49]] `screen.c`, [[source: kitty-child-monitor-0-49]] `child-monitor.c`, [[source: kitty-boss-0-49]] `boss.py`, [[source: kitty-history-0-49]] `history.c`, [[source: kitty-lineops-0-49]] `lineops.h` |
| wezterm | `b09b56c` | [[source: wezterm-screen-b09b56c]] `screen.rs`, [[source: wezterm-localpane-b09b56c]] `localpane.rs`, [[source: wezterm-gui-resize-b09b56c]] `resize.rs`, [[source: wezterm-issue-2162]], [[source: wezterm-issue-2987]], [[source: wezterm-issue-4995]], [[source: wezterm-issue-6669]], [[source: wezterm-issue-7853]] |
| VTE | 0.84.1 | [[source: vte-vte-cc-0-84]] `vte.cc`, [[source: vte-ring-0-84]] `ring.cc`, [[source: vte-internal-hh-0-84]] `vteinternal.hh`, [[source: vte-rewrap-txt-0-84]] `doc/rewrap.txt`, [[source: vte-issue-2154]], [[source: vte-issue-2294]] and [[source: vte-issue-2294-discussion]], [[source: vte-issue-2374]] and [[source: vte-issue-2374-discussion]], [[source: vte-issue-2701]], [[source: vte-issue-2919]] and [[source: vte-issue-2919-discussion]] |

VTE's bug 708213 is cited from its mention in `vte.cc`. The GNOME Bugzilla
record itself was not read.

## 0. The short answer

1. **Yes, reflow belongs in resize, and it should stay eager.** All six
   surveyed terminals re-cut inside the resize call. baud's own invariants
   need the re-cut and the geometry change to be one step, as seen from the
   parser and from every reader. The recorded resize rules assume it:
   - no timer that behaves like a debounce (`n-make-faster.md:75,90`);
   - SIGWINCH is sent on the drag event (`app.rs:4187-4193`);
   - the child is told the size the terminal settled on, and the `?2048`
     report follows the completed resize (`app.rs:4169-4186`,
     `term.rs:1134-1143`).
2. **What the owner is sensing is not the placement of reflow. It is the
   scroll position.** `resize_holding(ws, back)` takes a parameter and hands
   it back because the scrollback window lives in the app, while everything
   needed to anchor it lives in the core. Four of the six surveyed
   implementations keep the viewport in the layer that performs the re-cut
   and re-anchor it there. The one that keeps it in the GUI (wezterm) is the
   one whose viewport goes stale across a reflow.
3. **Two defects found by reading come from how resize is *structured*,
   not from where reflow runs.** Both sit at the seam between the primary's
   two width models. **Both are fixed in `27afdd9`**, each red first. A third
   item, DECSC not following a re-cut, is a policy gap rather than a defect,
   and it goes to the owner.

## 1. What the prior art does

### Table A: mechanics

| | Where the re-cut runs | How often resize is called | Eager or lazy | Storage; where the re-cut writes | Height | Reflow optional |
|---|---|---|---|---|---|---|
| **alacritty** | Inside `Term::resize` → `Grid::resize`: height first, then width (`grid/resize.rs:22-32`) | The library never coalesces; it returns early when dims are unchanged (`term/mod.rs:662-665`). The app keeps the last `Resized` size and handles it once per event-loop batch, with no timer. **[corr]** For one resize the terminal lock guarantees reflow happens before the child's redraw is parsed. Two things it does not order: old-size bytes still unread, and N+1's reflow re-wrapping N's redraw during a drag | Eager: every retained row is walked on each width change | Ring of rows, `WRAPLINE` on the last cell. Re-cut into a fresh `Vec` via `replace_inner` (`resize.rs:119,238`) | Separate routine, same call, before width; applies to both grids | No; #4424 and #4966 were closed |
| **foot** | Inside `render_resize` → `grid_resize_and_reflow` (`render.c:4706-4741`) | Every configure that changes cols or rows. During an interactive drag with `resize-delay-ms`>0 (default 100): a truncating snapshot is shown and the pty is paused, and TIOCSWINSZ plus the real reflow wait for a re-armed timer or the drag's end (`render.c:4577-4598`, `4278-4280`). **[corr]** On the timer path and some end-of-drag paths, TIOCSWINSZ precedes the reflow | Eager over the whole ring. Only the drag debounce defers it | Power-of-two ring of heap rows, `linebreak` flag. Re-cut into a freshly allocated ring (`grid.c:837`) | Same routine. The last reflowed line goes to the bottom (`grid.c:1203-1209`); #939 (open) concedes this is wrong after Ctrl+L | No |
| **ghostty** | Inside `PageList.resize` → `resizeCols`, on the IO thread under the terminal mutex (`PageList.zig:1288-1311`) | A 25 ms throttle, not a debounce: the latest size is applied once per timer (`termio/Thread.zig:390-396`). The pty size is set before the mutex is taken (`Termio.zig:504-513`) | Eager on every page, **[corr]** decompressing all of them. A TODO proposes deferring off-screen pages (`PageList.zig:1252-1254`), and one comment still says "lazily" | Linked list of pages of rows, `wrap`/`wrap_continuation`. Re-cut into fresh pages, recycling the source pages | Separate routine (`resizeWithoutReflow`'s rows branch), run before or after the width depending on direction | No config; it follows DECAWM (`Terminal.zig:4090`) |
| **kitty** | Inside `screen_resize` → `resize_screen_buffers` (`screen.c:666`, `315`) | OS-window resizes are debounced: 0.1 s after the end, 0.5 s on a pause, or the OS end event on macOS (`child-monitor.c:1334-1364`). **[corr]** During the debounce the old, un-reflowed content is still drawn, and the "N by M cells" banner is conditional. A split-border drag reflows at every cell step but holds SIGWINCH until the drag ends (`boss.py:2875,2900`) | Eager for main and history. The separate pager byte ring is rewrapped lazily at read (`history.c:568`) | LineBuf (`line_map`) plus a HistoryBuf ring, wrap bit on the last cell. Re-cut into fresh buffers | Same routine. Shrinking drops empty bottom rows, then retires from the top. Growing pulls history only with an option that defaults to off | No |
| **wezterm** | Inside `LocalPane::resize` → `Screen::resize` → `rewrap_lines` (`screen.rs:226-235`) | No coalescing: every OS resize event (`resize.rs:44`, **[corr]** plus two zero-size early-outs). The pty is resized first, then the lock taken (`localpane.rs:417-424`). **[corr]** Whether the child's redraw lands after the rewrap is a race with the parser thread; in practice it lands after | Eager: drains the whole `VecDeque` | `VecDeque<Line>`, `wrapped` attribute on the last cell. Re-cut into a fresh `VecDeque` (`screen.rs:109-176`) | Same function, but not rewrap: a window over the deque's tail with "bottom gravity", and blank rows below the cursor pre-pruned on every resize | No |
| **VTE** | Inside `set_size` → `screen_set_size` → `Ring::rewrap` (`vte.cc:8137-8138`) | No coalescing: every GTK allocation that changes the grid (`vte.cc:8561-8565`). The pty is set first, then the rewrap. A maintainer: "resizing-rewrapping is an atomic step" (#2919) | Eager. #2154 (open) proposes wrapping only the visible part on demand | The one hybrid: a thawed row array plus frozen text, attr and row streams. Text and attrs are left in place; only `row_stream` is regenerated, into a fresh stream (`ring.cc:1225`) | Same function; the rewrap is skipped. Rows below the cursor's paragraph are dropped, bounded (bug 708213) | Yes, but deprecated since 0.58 |
| **baud** | Inside `Term::resize_holding` (`term.rs:827-1161`). Primary screen only, and only in `Zone::Output` (`:1006`) | No coalescing, by owner rule: each drag step that changes the grid resizes, tells the pty and presents (`app.rs:4073-4217`). Sub-cell steps skip | Eager | One caller-provided ring of rows at a fixed pitch, wrap flag in `RowExtent`. Re-cut **in place** (`reflow.rs:1-34`) | Same function: rotation arithmetic over the ring (`term.rs:850-983`) | No switch; the zone gate |

### Table B: state

| | Viewport owner | Viewport across a width re-cut | Carried through | Dropped or reset | Alternate screen | Prompt (OSC 133) |
|---|---|---|---|---|---|---|
| **alacritty** | `Grid::display_offset`, one per grid (`grid/mod.rs:129-134`) | Approximate. Anchored to the bottom row: moved ±1 for each row added or removed below it, then clamped (`resize.rs:189-201`, `358-361`) | The cursor. **[corr, read]** When its cell merges up from Line(0) it is clamped to column 0. Hyperlinks and zero-width marks move in the cell's extra data | Saved cursor only clamped. Selection cleared on a column change. Scroll region reset. **[corr]** The app's search origin is neither remapped nor cleared | Truncated (`reflow=false`); no history | None: OSC 133 is unhandled. Maintainers call the duplication unfixable in the terminal (#2408, #4424) |
| **foot** | `grid.view`, one per grid (`terminal.h:233`) | Exact: a tracking point is carried cell by cell (`grid.c:874-882`), then clamped | Tracking points: cursor, saved cursor, selection ends, viewport. URI and underline ranges. **[corr]** `prompt_marker` is copied per character, so it is duplicated on narrowing and can be cleared on widening. Sixels | Search match not tracked. URL mode exited. Tabs reset. **[corr]** Blank rows directly above a bottom-row prompt are dropped in 1.21.0 (#2011, fixed in 1.22.0 by `c8470f40`) | Truncated (`grid_resize_without_reflow`); no history | None; relies on the shell's redraw. #939 (open) asks where the prompt should land |
| **ghostty** | `PageList.viewport` plus a pre-tracked pin (`PageList.zig:469-474`) | Exact: the pin moves cell by cell (`:1747-1757`), and snaps to `.active` if it lands there | Tracked pins: cursor, saved cursor, selection, kitty placements. `semantic_prompt` is copied onto every split row | Search is rebuilt. Scroll region, tabs and synchronized output are reset | Truncated; no history (`Terminal.zig:4094-4105`) | Re-cuts the prompt, then blanks from its start to the bottom (`redraw=true`) or just the cursor's row (`last`). **[corr]** After a narrowing the clear can start mid-prompt, because every split row is marked `.prompt` |
| **kitty** | `Screen.scrolled_by`, in the core | **Not re-anchored**: it keeps its number and is only clamped (`screen.c:4087`) | **[corr]** Cursors: the rewrap moves the main screen's; the alternate screen's are only clamped. `prompt_kind` goes to the first new row | Selections, URL ranges and the last-visited prompt are cleared. Ordinary image placements move only when the width is unchanged | Truncated. **[corr]** Its widen branch that extends the style is dead code | Blanks the prompt rows before the rewrap and pastes them back un-reflowed at the old width, truncated when narrower. Relies on the shell's redraw. `redraw=0` opts out |
| **wezterm** | **The GUI**: `PaneState.viewport: Option<StableRowIndex>` | **Stale** **[read]**. The rewrap never adjusts `stable_row_index_offset`, so the same number names other content (read from the code; draft PR #4995, open, reports the same staleness for kitty placements) | Cursor. The *inactive* screen's saved cursor. OSC 133, OSC 8 and image tiles travel in the cells; **[corr, read]** rows holding only an image are not re-cut and can be pre-pruned | The active screen's DECSC is not remapped (#6669, open). Kitty placements go stale (#4995). The selection goes stale, and is cleared only if dirty in the visible range | Truncated when narrower; only dirtied when wider | None. #2162 and #2987 are open. (#7853, unmerged, is about output cleared with Ctrl+L coming back on a resize, not about prompts.) |
| **VTE** | `VteScreen.scroll_delta` (`vteinternal.hh:159`) | Exact: the cell below the viewport's bottom-left corner is a marker carried through the rewrap, plus snaps to top and bottom (`vte.cc:8113-8195`) | Markers: cursor, saved cursor, selection ends, the end of the cursor's paragraph. Attributes, hyperlinks and OSC 133 bits stay in the untouched streams | Block selection deselected. Region and DECOM reset | Never rewrapped, and it cannot be: that ring has no streams | None: "There's not much VTE could do here" (`doc/rewrap.txt:189-193`). #2919, #2294, and #2374 (**[corr]** the final diagnosis is rewrap, not timing) |
| **baud** | **The app**: `App::back` (`app.rs:341-345`) | Exact, but only because the app passes it in. `resize_holding` maps the window's first row through the re-cut and returns it (`term.rs:1039-1091`) | The cursor (the primary's own, or `alt_saved`). Images, via the `Follow` hook. The prompt block, as a ring row | DECSC (`cold.saved`) is never remapped; it is clamped at restore (`ops.rs:67-70`). The app clears the selection, predictions and slide (`app.rs:4146-4198`). Margins reset | Cell-addressed at the fixed pitch: narrower hides columns, wider blanks the revealed ones (`widened`). D1 was here, fixed in `27afdd9` | In `Zone::Prompt` the primary is not re-cut at all. The block is tracked as a ring row and replaced at the redraw mark |

### What the tables say

1. **Every one of the six re-cuts inside the resize call.** They differ in
   how often resize is called:
   - two debounce: foot's drag, and kitty's OS window;
   - one throttles: ghostty, at 25 ms;
   - one keeps the last size per event batch: alacritty's app;
   - two do nothing: wezterm and VTE.

   baud's rule puts it with the last two.
2. **None of the six re-cuts lazily.** The near misses:
   - ghostty's TODO for off-screen pages;
   - VTE's open #2154;
   - VTE's hybrid, which keeps text and attributes as logical lines but still
     regenerates an O(history) row index on every width change: "resizing 1
     million lines take about 0.2 seconds" (`doc/rewrap.txt:434-439`);
   - kitty's secondary pager ring.
3. **baud is the only one that re-cuts in place.** Every other
   implementation copies into fresh storage. `reflow.rs:8-12` says the same.
4. **The viewport correlation is the finding that bears on the owner's
   question.**
   - ghostty, foot and VTE re-anchor the viewport to content exactly, and
     alacritty approximately. All four hold it in the layer that performs
     the re-cut.
   - kitty holds it there and does not re-anchor it.
   - wezterm holds it in the GUI, and it goes stale **[read]** (#4995
     shows the same for kitty placements).
   - baud holds it in the app but anchors it exactly, which works only
     because resize takes it as a parameter.
5. **Nobody reflows the alternate screen.**
6. **Prompts get three treatments:**
   - Two terminals act on the prompt: kitty keeps its rows out of the
     rewrap, and ghostty blanks them after it. Both still rely on the
     shell's redraw, and both have a failure mode. kitty truncates the
     pasted-back rows. ghostty's clear can start mid-prompt **[corr]**.
   - Four give up.
   - baud's zone gate is a third approach. The open item in the M note
     (`m-resize-and-reflow.md:166-175`), re-cutting the output above a
     prompt while leaving the prompt's rows alone, is kitty's shape.
7. **Selection across a width change is handled three ways:** cleared by
   alacritty, kitty and baud; remapped by foot, ghostty and VTE; left stale
   by wezterm.

## 2. The question underneath

"Should reflow be in resize?" bundles five questions, and they have
different answers.

1. **Where does the re-cut run?** Answer: inside resize. In baud this is not
   a matter of taste:
   - **Row state depends on the width.** `RowExtent` marks and the wrap flag
     are set relative to the width a row was written at, and `reflow`
     assumes every source row is `old` wide (`src` uses `c.old`). A feed
     between a geometry change and a pending re-cut would write rows at one
     width next to rows cut at another.
   - **The `?2048` report must follow a finished resize**
     (`term.rs:1134-1138`), and the app flushes it after the ioctl
     (`app.rs:856-876`).
   - **Every reader is `&self`** (`Term::view`, `term.rs:1273`). A pending
     re-cut could not be forced by a read without making reads `&mut` or
     adding interior mutability.
   - **Parse and re-cut must not interleave.** VTE and alacritty guarantee
     this with their structure and their lock.
2. **Who owns the scroll position?** Today the app does. That is the whole
   reason `resize_holding` exists, and the reason the core exports
   `history_mark`, `history_moved_since` and `looking_back`.
3. **Is the re-cut eager, deferred or lazy?** Today: eager, at each drag
   step that changes the grid, gated by the primary's zone.
4. **Which content is re-cut?** This question is baud's alone. The primary
   has two width models on one ring:
   - **Re-cut in `Zone::Output`.** A row's cells are a line's cells at the
     current width.
   - **Cell-addressed in `Zone::Prompt`.** A row keeps what was written, at
     whatever width, and the live width hides or shows it.

   Each model is sound on its own. None of the six gates its primary model
   on OSC 133 state, so the seam between the two is new ground, and D1 and
   D2 below live on it.
5. **How often does the embedder call resize?** This is settled by owner
   rule: no debounce, and SIGWINCH goes out on the drag event.

So the thing that looks wrong, a parameter going into resize and a mapped
value coming out, is a question-2 problem. It is not a question-1 problem.

### Two defects in resize, from its structure, and one policy gap

Found by reading at `1413048`. Both defects are **fixed in `27afdd9`**, each
proved red first; the tests are named below.

**D1. The alternate screen's widen is gated on the primary's zone.**
- **Where:** `widened` (`term.rs:1106-1117`) is nested inside
  `if cols changed && self.primary_zone() == Zone::Output` (`:1006`).
- **Trigger:** widening while the alternate screen is up and the primary
  sits in `Zone::Prompt`. That happens when a shell widget runs a
  full-screen program, the shape "Each screen has its own prompt state"
  records.
- **Effect:** the revealed columns are not blanked, so the frame the
  program drew when it was last this wide shows under the new one. That is
  the "two images at once" case the blanking exists to prevent.
- **Fixed:** `widened` runs whatever the primary's zone
  (`the_alternate_screen_grows_whatever_the_shells_zone`,
  `a_wider_alternate_screen_blanks_what_it_reveals_while_the_shell_is_at_a_prompt`).
  The same gate also kept a growth at a prompt from raising any mark on the
  *primary*, which `27afdd9` found and `View::settle_growth` fixes
  (`at_a_prompt_a_wider_screen_still_erases_a_band_to_the_old_edge`).

**D2. A narrowing re-cut leaves other lines' bytes past the new width.**
- **Mechanism:** `write_ord` addresses `d % c.new` (`reflow.rs:173-176`),
  and `finish_line` fills the tail only to `new` (`:188`) while setting the
  row's mark at or below `new`. Columns `[new, old)` of each destination row
  keep whatever that physical row held before the copy.
- **Invariant broken:** the fixed pitch rests on "a row's mark bounds its
  content over the whole pitch" (CLAUDE.md, "A resize moves no cell").
- **When it shows:** a widen that re-cuts rewrites the row to the new
  width and cleans it. A widen in `Zone::Prompt` neither re-cuts nor calls
  `widened` on the primary, so it would expose those bytes. Example: narrow
  while a build scrolls, then widen at the prompt.
- **Corroboration:** the full-reset commit (`5e00c08`) had found the same
  mechanism for rows past the screen after `RIS` and fixed that one path with
  `erase_ring`.
- **Fixed:** `finish_line` blanks every row it writes from the new width to
  the pitch (`a_narrowing_re_cut_leaves_nothing_past_its_width`,
  `a_tail_hidden_at_a_prompt_is_dropped_cleanly_by_a_later_re_cut`).

**G3 (a policy gap, not in the fix-first set). DECSC is not carried
through a re-cut.**
- **Where:** `cold.saved` is untouched by resize and only clamped at
  restore (`ops.rs:67-70`).
- **Effect:** after a narrowing re-cut, the saved row and column name a
  cell that now belongs to another line. This is wezterm's #6669 class:
  "losing shell text because the restored cursor is put in a place where the
  shell overwrites prior output".
- **Others:** foot, ghostty and VTE carry the saved cursor through, and
  kitty carries the main screen's. alacritty only clamps, as baud does.
- **Why it is the owner's:** clamping is a defensible choice that alacritty
  shares, and fixing it needs either B+ below or an ad-hoc marker through
  `Follow`. It is Q4 in §5.

**Also by reading:** `baud-core/tests/altresize.rs` provisioned exactly 50
rows and discarded `Resized` with `let _ =`, so its growth was truncated and
the test could not fire. Fixed in `27afdd9`: it provisions sixty rows and
asserts `Ok`, and its primary half is rewritten to the rule in force since
2026-09-18 (question 6).

## 3. Options

### (A) Status quo: `resize_holding(ws, back)`

- **Constraints:** all satisfied. No state is added to the core.
- **API:**
  - two entry points;
  - `back` written at six app sites (`app.rs:802`, `2775`, `3110`, `3721`,
    `4047`, `4158`);
  - anchoring helpers `anchored`, `after_read` and `resized_window`
    (`app.rs:1785-1894`);
  - a source gate that lists every raw read (`app.rs:2171ff`).

  The Linux driver has no window over the history at all: it lacks "the
  wheel onto the scrollback" (`linux.rs:17-18`). When it gets one, it
  repeats all of this.
- **Recorded bugs in this class:** each was an app integer anchored by a
  rule other than the core's numbering, and each was fixed by adding a core
  accessor or parameter:
  1. A zoom that shrank the grid added the retirement to a live `back`, and
     the banner appeared four times (`app.rs:1762-1783`).
  2. The alternate screen's output pushed the window up the primary's
     history, and `moved_since` taken across two rings panicked ("A window
     over the history is the primary's").
  3. Anchoring by the top line's delta moved the window from `s 05` to
     `s 00` ("A re-cut holds a window...").
- **What comes next:** anything else that should follow content needs
  another parameter or accessor: a selection that survives a resize, a
  search hit, a second view, DECSC.
- **Cost:** none.

### (B) The core owns the window

- **Shape:** `Book` gains `window: Option<LineIdx>`: the absolute line at the
  view's top, with `None` meaning live. It uses the same numbering images
  use (`Book::top`, `storage.rs:220-228`).
- **Why it is simpler:** an absolute line does not move when output
  scrolls, when a height step retires or pulls rows, or when the prompt
  block retreats. `top` moves; the line stays put.
  - This is exactly today's `held` arithmetic (`term.rs:959-972`) and
    `after_read`'s, stated once.
  - The re-cut maps it with the `Follow` hook that already maps images and
    `held` (`term.rs:1037-1091`).
  - The alternate screen cannot move it, by construction: `Book::top` is the
    primary's.
- **It is not free: the anchor has to be normalized.** An anchor that is
  only clamped when read is wrong in two ways:
  - **It has reached the live screen.** The owner's rule is that a live
    window stays live (`app.rs:1762-1783`). An anchor that reached `top` and
    was left there would, after a later scroll, make a live window look
    scrolled back again.
  - **It has fallen below the floor.** Something shrank the history, and
    later output grows it back. The window would then pin to the oldest
    line when it should be live or on its line.

  The app avoids both today because `after_read` writes its clamped value
  back on every read (`app.rs:1880-1883`).

  Several paths move `top` back, or shrink `hist`, under an anchor:
  - a height-growth pull (`term.rs:926-937`);
  - the prompt retreat (`replace_prompt_block`, `prompt.rs:89ff`);
  - a full-screen scroll down on the primary (`unretire`,
    `ops.rs:238ff`);
  - a re-cut whose held row lands on the live screen
    (`term.rs:1086-1091`);
  - `RIS`, which empties the history.

  `ED 3` (erase saved lines) would be another; `EraseExtent` has no such
  variant today (`paint.rs:902-918`).

  Enumerating them is the fragile form. The robust form: normalize once, at
  the exit of `feed` and of `resize`. If the anchor is at or past `top`, it
  becomes `None`. If it is below the floor, it becomes the floor, or `None`
  when `hist` is 0. That is O(1) per call, not per byte, and it reproduces
  `after_read` and `resized_window` exactly, including the net movement
  within one read that they compute today.
- **Constraints:**
  - `no_std` and alloc-free: one field in caller storage.
  - `Paint` carries `&mut Book` (`paint.rs:156`), so `Paint`'s layout does
    not change, and the field is not written per scroll. The normalization
    runs once per `feed` call, on the entry point, not in the per-byte loop.
    The ingest cost is argued to be nil, not measured; rules #1 and #2 say a
    sweep is owed.
- **API:**
  - `Term::resize(ws)` becomes the only entry point, and `resize_holding`
    goes.
  - New calls: `view()` reads the core's window; methods scroll it by
    lines, set it (the thumb), send it live, and read its depth.
  - A `view_at(back)` can stay for a caller that wants an arbitrary view.
  - `looking_back` becomes the internal clamp.
  - By reading of `app.rs:1800-1894`, the window is the app's only consumer
    of `history_mark` and `history_moved_since`. Core tests pin them, so
    whether they go is a separate call.
  - `moved_since` and `screen_epoch` stay, for the selection and the slide.
  - The Linux driver gets a window over the history without repeating any
    anchoring.
- **Owner rules:**
  - "We should not change the scroll ever" is enforced in one place.
  - "A key snaps only the live screen's window" (after foot) stays the
    embedder's decision, now expressed as a call rather than an integer
    write. The core owns the mechanism and the embedder keeps the policy.
- **Prior art:** this is the shape of ghostty's `viewport_pin`, foot's
  viewport tracking point and VTE's `below_viewport` marker. Each is a
  position held by the layer that re-cuts, and mapped by the re-cut.
- **Risk:** one window per `Term`. Two views over one session need (B+).

### (B+) Tracked anchors

- **Shape:** a caller-provided, fixed-size array of anchors in `Storage`,
  each a screen, a `LineIdx` and a column. Every event that moves rows maps
  them: the re-cut's `Follow`, `RIS`, a surface switch. The window is one
  anchor. Selection ends, a search hit and DECSC can be others.
- **Prior art:**
  - ghostty's tracked pins: cursor, saved cursor, selection, kitty
    placements, viewport;
  - foot's tracking points;
  - VTE's markers.

  baud already has two ad-hoc instances, image staging and `held`, and the
  hook they share.
- **Consequences:**
  - Carries DECSC through a re-cut, if the owner wants it remapped (Q4).
  - Lets a selection survive a resize, if the owner wants that.
  - Merges the app's `Scrolled` numbering (`select.rs:50-55`) with the
    core's `Book::top`. Today there are two numberings, and only the core's
    follows a resize.
- **Cost:** one array, one walk per re-cut (N is small and the path is cold),
  and a registration API. A full array refuses and counts, as the house
  style does.

### (C) The re-cut as a separate step

**The honest argument for it.** An eager re-cut through an intermediate
narrow width loses history.
- `reflow` drops whole lines from the oldest end when the re-cut exceeds
  the ring (`reflow.rs:309-313`), and widening back does not bring them
  back. CLAUDE.md: "A reflow that drops is not an identity".
- So a drag from 200 columns to 40 and back, over a full ring, loses what
  the 40-column cut could not hold. How much is **unmeasured**.
- VTE documents the same row-capped limit: "The scrollback buffer also
  remains defined in terms of lines" (`doc/rewrap.txt:289-291`). #2154 (open)
  proposes fixing it by counting scrollback in paragraphs. **[corr]** VTE's
  #2701 is not this limit: it reports a scrollbar-jump regression, and the
  line loss is only its setup.

Coalescing to the final size would avoid the loss. Each form of it fails
here:

- **(C1) An explicit `Term::reflow()` after `resize`.** Between the two
  calls, geometry and ring disagree. A feed in that window writes at the new
  width next to rows cut at the old one, and `reflow` assumes a single
  source width. It is an API foot-gun unless it is enforced, and enforcing
  it is C2.
- **(C2) Pending until the next access.** `feed` would apply a pending
  re-cut first, so the child's redraw is still parsed after it. But readers
  are `&self`, and `relayout` presents a frame at every step that changes
  the grid (`app.rs:4217`), so a read forces the re-cut at every step
  anyway. In practice it is eager, and it costs interior mutability or
  `&mut` readers besides.
- **(C3) Debounce or throttle, as foot, kitty and ghostty do.** The
  recorded rules forbid it:
  - "a debounce on resize | the freeze was a deferred SIGWINCH, not a
    missing timer" (`n-make-faster.md:90`);
  - "no timer that behaves like a debounce" (`:75`);
  - SIGWINCH is sent on the drag event (`app.rs:4187-4193`).

  foot's form pauses the pty behind a truncated preview. kitty's shows the
  old content un-reflowed **[corr]**.
- **(C4) Cell-address during the drag and re-cut once when it ends.** The
  end would be signalled by an OS event, not a timer; kitty's macOS path
  uses the OS resize-complete event (`child-monitor.c:1341`). This avoids
  both the intermediate loss and the timer. It has a blocker: rows written
  during the drag are cut at mixed widths, and `reflow` needs a single
  source width. It would need a width per row. And the per-step SIGWINCH
  redraws would be re-wrapped by the final re-cut, which is alacritty's
  #2408 mechanism. Unexplored; recorded here so it is not re-derived.

**Verdict:** C1 and C2 buy nothing under baud's rules, a recorded rule
forbids C3, and C4 has a structural blocker. The re-cut stays inside
resize. The loss through narrow widths is a real trade-off, and it goes to
the owner (Q5).

### (D) Storage by logical line, wrapped at read time

- **Prior art:** nobody ships it.
  - VTE's frozen streams are logical: text only carries `\n` at hard ends
    (`ring.cc:376-378`). It still regenerates an O(history) row index on
    every width change. #2154 is an open proposal.
  - kitty's pager ring is the only store rewrapped at read time, and it is
    a secondary store.
- **Consequences for baud:**
  - It runs against the owner's "grid as ring, zero copy".
  - Everything that addresses by row would have to change: `RowExtent`,
    images, the prompt block, cluster ids in cells, row damage, the
    per-frame window copy.
  - Lines of variable length need arena management in caller storage.
  - The nearest measured precedent, the span model, was +50% on two
    recordings and -5 to -10% on six, just to carry the field. It was
    reverted.
  - Random access by screen row needs a row index, which is exactly VTE's
    regenerated `row_stream`.
- **What it would buy:** history capacity counted in lines rather than
  rows, so nothing is dropped on narrowing; and, with a lazy index, a width
  change that costs O(visible).
- **Verdict:** this replaces the architecture. It does not answer the
  question.

### (E) Restructure resize, with no API change

- **(E1) Split `resize_holding` (330 lines) into private per-screen
  steps, each with its policy stated:**
  1. primary height;
  2. primary width, gated by zone;
  3. alternate width, not gated by the primary's zone (this fixes D1);
  4. cursor and margins;
  5. anchors;
  6. the report, last.

  It makes the two-width-model seam visible in code. The path is cold.
- **(E2) Re-cut by prompt scope (the M note's open item).** Re-cut
  everything except the prompt block's rows. kitty does this and truncates
  those rows when narrowing (`lineops.h:39`); baud's fixed pitch could hide
  them instead. It needs the in-place ordering argument that the M note says
  `reflow` does not carry today. It would narrow the seam D2 lives on. It is
  a separate change.

## 4. Recommendation

1. **The re-cut stays inside resize, and stays eager.** This is the direct
   answer to the owner's question.
   - All six prior-art implementations do it.
   - baud's invariants need it (§2, question 1).
   - The owner's rules on SIGWINCH and debounce assume it.
   - The cost is bounded by `Depth::DEFAULT` (2,000 lines;
     `history.rs:131`). Two scratch timings of a full-ring narrow-and-widen
     round trip disagree by about 2x: ~0.6 ms plus ~0.7 ms
     (CLAUDE.md:11969-11972), against 654.5 us for the whole round trip
     (:17180-17184). A third scratch bench, taken for `27afdd9`, reads the
     narrowing at 299 us and the widening at 347 us before that change, so
     it agrees with the second. The ~0.6 ms plus ~0.7 ms is the odd one out
     and is unexplained. One candidate: a lone, non-interleaved run of the
     third bench's changed build read 457 and 597 us, where the interleaved
     sweep reads 317 and 412, so a run by itself has read high at least
     once. None
     of the three benches is kept runnable. After `27afdd9` the third reads
     317 us and 412 us, ranges disjoint from before, and a 100-column growth
     at a
     prompt, which cost nothing, costs 46-54 us (CLAUDE.md, "A width growth
     keeps the marks honest at a prompt").
   - Row 10's per-step figures (0.19-0.56 us; a width change in
     `Zone::Output` 1.5-7.0 us) come from fixtures with 0-19 retained
     lines, and are from before `27afdd9`: since, `live-drag`'s resize is
     1.44 us a step. The `Zone::Output` control moved on both gestures,
     `drag` 5.67 to 7.02 us and `live-drag` 4.88 to 6.58, disjoint
     (interleaved, at one build path; a first reading from one launch a
     build had `live-drag` at 2.28 and its control overlapping).
   - A full-ring drag in `Zone::Output` is **unmeasured**, and so is any
     depth past the default. The cost scales with the ring.
2. **~~Fix D1 and D2 first, each red before it is fixed.~~ Done in
   `27afdd9`.** E1, splitting `resize_holding` into per-screen steps, was
   not done with them: the fix moved the alternate screen's `widened` out
   of the zone gate, which is the nesting bug, added `View::settle_growth`
   for the primary beside it, and left the function whole. G3 waits for
   Q4.
3. **Move the window into the core (B).** Ownership should follow
   knowledge:
   - The core already holds the numbering (`Book::top`), the clamp
     (`looking_back`) and the mapping (`Follow`). The app holds one
     integer.
   - The prior-art correlation in §1 point 4 points the same way.
   - Each of baud's three recorded bugs in this class came from anchoring
     an app integer by a rule other than the core's numbering. Under B there
     is one numbering and one owner.

   Offer B+ as the owner's fork: it is the right shape if selection or DECSC
   should follow a re-cut, or if one `Term` may ever show two views.
4. **Migrate B as a differential.** Keep the app's `anchored`,
   `after_read` and `resized_window` as the oracle during the move. Run a
   property test over random sequences of feed, wheel, key and resize,
   including reverse scrolls, prompt redraw marks and `RIS`. It requires the
   core's window to equal the oracle's `back`. Delete the oracle only once
   they agree. That test is the check that normalizing at the exit covers
   every path that moves `top`.

**What would prove this wrong:**

- **Against "inside resize, eager":** a committed bench (`winch` cost with a
  full-ring `Zone::Output` payload) showing a drag step that changes the
  grid over the frame budget (8.33 ms at 120 Hz) at the depth the owner
  picks. Or evidence that users lose history they want through intermediate
  narrow widths. Either would revive C4 or D.
- **Against B:**
  - the owner wants two views over one `Term`, which means B+ or leaving the
    window outside;
  - a sweep shows the `Book` field costing ingest. The layout class is real
    here: `cat` read about -8% on a change that ran none of its code (the
    region-image section);
  - some path moves `top` or `hist` in a way that normalizing at the exit
    of `feed` and `resize` does not reproduce. The differential in step 4
    finds that.
- **Against D2:** its red test went red on the code this note read, so
  this one did not happen.

## 5. Decisions that are the owner's

**Answered 2026-09-25, through note V.** Note V
(`docs/research/notes/v-open-decisions.md`) put these nine to the owner as
its V2 to V9: questions 1 and 2 as V2, then question *n* as V*n*. Its
"Decisions taken, 2026-09-25" records the answer: "**Every other decision:
as recommended.** V1-V9, V11, V15, V17, V20-V22 and V25." Under each
question is the recommendation that answer took, quoted from note V's
section for it.

1. **Should the core own the scrollback window (B)?** For: one anchoring
   rule, an end to the recorded class of anchoring bugs, and a Linux window
   for free. Against: view state enters a core described as "VT finished",
   an API change, and a normalization step on `feed` that the differential
   has to prove.

   *Answered: note V's V2, as recommended.* "**Yes to B: the core keeps the
   view as an absolute line (or `Live`), and the app keeps only the policy of
   when a key snaps it live. No public tracked-point array yet.**"
2. **One window, or tracked anchors (B+)?** B+ supports two views on one
   session, selections that survive a resize, and a DECSC that follows. It
   costs a bounded array and a registration API.

   *Answered: note V's V2, as recommended.* The recommendation under question
   1, whose last sentence answers this one: "No public tracked-point array
   yet."
3. **A selection across a width change: clear or remap?** Clearing is what
   alacritty, kitty and baud do today, and it is simple. Remapping is what
   foot, ghostty and VTE do; it needs B+. wezterm leaves it stale, which is
   the outcome to avoid.

   *Answered: note V's V3, as recommended.* "**Yes: on the main screen, keep
   the selection across a height-only resize by renumbering it. Clear it on
   any column change, on any resize while the alternate screen is up, and when
   it touches rows the shrink cut (lean; clipping is the alternative). No
   remap through a re-cut for now.**"
4. **DECSC across a re-cut: remap or clamp? (G3)** foot, ghostty and VTE
   remap it, and kitty remaps the main screen's. baud and alacritty clamp.
   With clamping, a restored cursor can overwrite the output the shell wrote
   (wezterm #6669). Remapping needs B+, or an ad-hoc marker through
   `Follow`.

   *Answered: note V's V4, as recommended.* "**Yes: the ESC 7 position should
   follow its text through a resize. Then a fork, (i) or (ii). My lean is (i),
   provided xterm's cursor.c, pinned first, agrees with ghostty and alacritty;
   if xterm keeps the ?1049 save separate, take (ii).**"
5. **Depth against eagerness, and loss through narrow widths.** A bounded
   default depth keeps the eager re-cut cheap. A much deeper or infinite
   history would eventually push toward lazy wrapping (D), which nobody
   ships. Separately: accept that a drag through a narrow width drops old
   lines, as every eager terminal does, or pursue C4.

   *Answered: note V's V5, as recommended.* "**Yes: keep the re-cut eager and
   the depth at 2,000 lines (fixed by the L2 scroll cost), count the rows a
   re-cut drops, and commit a full-ring fixture that measures a drag step.**"
6. **Which rule governs the top line across a resize?** The record carries
   a sequence, the code follows the last, and the record calls the last the
   owner's rule for the primary since 2026-09-18. Confirm it.
   1. "when resizing the current first row changes and it should not -- we
      should not change the scroll ever, we should only re-arrange the grid
      of what is visible, not go to history" (`app.rs:1767`,
      `altresize.rs:3-6`);
   2. "A resize never changes the top line. Only feeding bytes may."
      (`resize_recorded.rs:18-20`);
   3. the refinements: "what the first line is changes when the content
      starts to push up and exceed the visible side at the current window
      size" (`resize_recorded.rs:357-360`); content that no longer fits
      pushes into the history (`term.rs:872-880`); "pull down to the bottom,
      and blank-fill only when nothing is left to pull" (M note 306-308,
      `term.rs:900-903`).

   `27afdd9` found that `altresize.rs`'s primary test, which encoded the
   first, had never been able to fail, and rewrote it to the third, which
   its record calls the owner's rule for the primary since 2026-09-18. If
   the owner means the first, that test is the one to flip.

   *Answered: note V's V6, as recommended.* "**Yes: confirm rule 3. Rule 1
   survives as the rule for a scrolled-back view: a view up in the history
   holds its line, and a live view stays live.**" Note V's design for it: "No
   code."
7. **Re-cut by prompt scope (E2)?** It re-wraps long lines of finished
   output while the user sits at a prompt, and it narrows the D2 seam. It
   costs a new in-place ordering proof.

   *Answered: note V's V7, as recommended.* '**Not now: keep "no re-cut at a
   marked prompt", and revisit after V2, V9 and V5's full-ring bench.**'
8. **Resize cadence.** The rule of no debounce and a SIGWINCH per step
   stands on a measured freeze, while four of the six prior-art terminals
   coalesce in some form. Confirm the rule knowing what it costs: an eager
   re-cut at every step (unmeasured at full depth) and loss through
   intermediate widths (Q5).

   *Answered: note V's V8, as recommended.* "**Yes: confirm no debounce, and a
   SIGWINCH plus a resize on every drag step that changes the grid.**" Note
   V's design for it: "No code."
9. **What does a cut row have to hold to count when a growth reveals it?**
   This was found after the draft, when `27afdd9` made the marks honest. The
   height step reveals the cut rows "as far as the last one that holds
   anything", and it asks each row by reading the row's mark. A mark says
   where a row stops being uniform, so the test misfires in both directions.
   It does so on the code before `27afdd9` as much as after:
   - A row of blanks in a foreground-only rendition counts. It is revealed
     while a history line was left to pull.
   - A row erased whole in a visible background colour has a mark of zero,
     so it does not count. A history line is pulled over it.

   Honest marks add a third shape, and the owner's own recorded drag shows
   it on steps 641-654: an invisible band past the old edge counts once a
   width growth has raised its mark. All three are pinned in
   `scrollback.rs`, as they behave today. A definition by what a row *shows*
   would also stop counting rows of written spaces, which the mark counts
   today. So it needs its own sweep over the seven recorded gestures before
   it is chosen.

   *Answered: note V's V9, as recommended.* "**Yes to B: a cut row comes back
   when it holds a written character (a space counts, colour alone does not),
   tested by the same function the prompt retreat uses -- after the
   seven-gesture comparison below confirms which of your recorded resizes
   change.**" "Below" is note V's V9 section.
