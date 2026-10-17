# AQ. How the documentation is arranged, and for whom

**Status: the layout is built, 2026-10-16; the items under "Not done
here" are open.** The owner, the same day: "do a proper doc
cleanup and product focus, its now all over the place , it should be well
structured"; "the speed stuff on main page seems it should be in performance
doc , also i don't see feature list , configuration file doc etc";
"embedding the engine shoudl be its own doc no?"; "its like you are still
technical". This note is the survey behind the layout that answered them.
The ten projects' documentation was read at their repositories and sites on
2026-10-16; none of it is pinned in the manifest, because it is evidence of
practice, not a standard.

## What ten terminals do

ghostty, kitty, alacritty, wezterm, foot, contour, rio, iTerm2, Warp and
Windows Terminal:

* **The README is one of three shapes.** A signpost under 50 lines that
  points at a site (kitty, wezterm, rio); a pitch with facts, 100-260 lines
  (alacritty, contour, Warp, iTerm2, ghostty); or the manual itself (foot,
  Windows Terminal). Star count does not predict the shape.
* **Navigation is split by reader** where a project has a site: install,
  configure, features, reference, help (wezterm, ghostty, kitty).
* **A features page or list** exists in contour, foot and alacritty
  (a features page in its repository); the signposts leave it to the site.
* **A configuration reference** is generated from code in kitty, ghostty and
  contour, held to the code by a check in Warp, and hand-written with no
  check in the other six.
* **Help written as symptoms** ("I catted a binary file and now kitty is
  hung?") is in ghostty, kitty and foot.
* **Docs fail the build on a broken link** in kitty, wezterm and contour;
  none of them checks a claim against the code.

## What baud had

One README carried the pitch, the ingest table, what is built, the build
commands, the embedding example and an index that listed agent notes and the
2.8 MB record beside the coverage table. BUILD.md mixed running the app with
the suites. The only configuration document was `config.toml.example`, and no
page said what a user gets. The ingest table and the build were explained in
three places each.

## What was built

* **Re-index, and move only what is cheap to move.** `docs/vt-coverage.md`,
  `docs/input-coverage.md`, `docs/protocols/`, `docs/cursor*`, the record and
  `MEASUREMENTS.md` are read by tests at fixed paths and cited by dozens of
  files, so they stay; `docs/README.md` files them by reader. BUILD.md,
  `docs/architecture.md` and FUNDING.md had a handful of inbound links each,
  and moved.
* **Five reader groups** in `docs/README.md`: Use baud, Embed, Contribute,
  Reference, History and research.
* **A user guide** in `docs/guide/`, written for a person at a terminal,
  benefit first: features, install, configuration, status, performance,
  licensing. Technical detail is linked, not inlined.
* **The configuration page is held to the reader in both directions**, the
  Warp practice made a test: `the_configuration_page_lists_every_key_the_reader_knows`
  reads `Config::apply`'s arms and the page's tables. The example file was
  already held in one direction.
* **Embedding has its own page**, `docs/embedding.md`, and the compiled
  quickstart block is checked there instead of in the README.
* **History keeps its words.** A backticked path in the record or a note
  names the file as it was that day; `docs.rs` resolves a listed move for
  those two places only.

## Not done here

* The README's rewrite, a separate marketing pass; it keeps its content
  and only its links followed the moves.
* A screenshot of a working session on the first screen, which six of the
  ten show; a help page written as symptoms; GitHub's description and
  topics, which are the owner's to set.
* No site generator: GitHub renders the Markdown, and `docs.rs` already
  gives what `strict: true` gives the MkDocs projects, and more.
