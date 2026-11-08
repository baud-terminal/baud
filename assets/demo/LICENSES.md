# The films' pieces and their licences

baud's demo films two things: the tour, an MP4 for YouTube, and the trailer,
the README's film, a GIF of the tour playing. This file lists every piece the
tour shows and the licence each comes under. No trailer has been filmed yet,
so none is in the repository; its row is added here once its take exists.

Each third-party licence below was read at the URL given, on 2026-11-07 or
the date its section gives. For the fonts the demo fetches, a copy of each
licence file read is kept in `docs/research/sources/`, with its entry in
`docs/research/sources/MANIFEST.toml` (group FM) and its row in
`THIRD_PARTY.md`. A source marked `external` there was read and hashed but is
not kept. The fonts embedded in the app keep their notices beside them under
`baud-app/fonts/`.

## Noto Color Emoji

- **Piece:** the emoji and the flags. Fetched when the demo runs, from
  <https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/NotoColorEmoji.ttf>
  (tag v2.051, commit 8998f5d), sha256
  `72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b`. Not in
  the repository.
- **Licence:** OFL-1.1.
- **Read at:** `fonts/LICENSE` at that commit,
  <https://raw.githubusercontent.com/googlefonts/noto-emoji/8998f5dd683424a73e2314a8c1f1e359c19e8742/fonts/LICENSE>
  (`[[source: noto-emoji-fonts-license]]`). Read 2026-11-07.
- **Which licence file covers the font:** the repository has two. The
  README at v2.051 (`[[source: noto-emoji-readme-v2-051]]`) says: "Emoji
  fonts (under the fonts subdirectory) are under the SIL Open Font License,
  version 1.1." It links `fonts/LICENSE`, the file beside
  `fonts/NotoColorEmoji.ttf`. So `fonts/LICENSE` covers the font file. The
  README says the root `LICENSE` is the Apache License 2.0 for tools and
  images. At this commit the root `LICENSE` is in fact the OFL 1.1 too, with
  "Copyright 2013 Google LLC". It is byte for byte
  `baud-font/tests/fonts/NotoColorEmoji-LICENSE.txt`.
- **What it says:** "Permission is hereby granted, free of charge, to any
  person obtaining a copy of the Font Software, to use, study, copy, merge,
  embed, modify, redistribute, and sell modified and unmodified copies of the
  Font Software, subject to the following conditions". Condition 5 ends: "The
  requirement for fonts to remain under this license does not apply to any
  document created using the Font Software."
- **Kept copy:** `docs/research/sources/noto-emoji-fonts-LICENSE-8998f5d.txt`.
- **YouTube:** allowed. The OFL has no NonCommercial term and no
  NoDerivatives term. The video shows text drawn with the font and does not
  carry the font file.

## Noto Sans CJK SC

- **Piece:** the Chinese, Japanese and Korean lines. Fetched when the demo
  runs, from
  <https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf>
  (tag Sans2.004, commit 523d033), sha256
  `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. Not in
  the repository.
- **Licence:** OFL-1.1.
- **Read at:** the repository's `LICENSE` at that commit,
  <https://raw.githubusercontent.com/notofonts/noto-cjk/523d033d6cb47f4a80c58a35753646f5c3608a78/LICENSE>
  (`[[source: noto-cjk-license]]`). Read 2026-11-07. `Sans/` has no
  `LICENSE` of its own.
- **What it says:** the OFL 1.1, the same grant and the same condition 5 as
  Noto Color Emoji's. The file carries no copyright line.
  `Sans/README-third_party.md` (`[[source: noto-cjk-sans-readme-third-party]]`)
  says "License: SIL Open Font License v1.1" and "License File: LICENSE", and
  that the licence "was changed to the SIL OFL v1.1 starting with Version
  1.002". Its description paragraph still says "released as open source
  under the Apache license version 2.0". It also says "The copyright is held
  by Adobe, while the trademarks on the names are held by Google."
- **Kept copy:** `docs/research/sources/noto-cjk-LICENSE-523d033.txt`.
- **YouTube:** allowed. No NonCommercial or NoDerivatives term, and the
  video does not carry the font file.

## Noto Naskh Arabic

- **Piece:** the Arabic line. Fetched when the demo runs, from
  <https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Regular.ttf>
  (tag noto-monthly-release-2026.10.01, commit 0259702), sha256
  `c34fdbd98af4dbc45ca192a23d2eeb77032add83086f5fe32957d23b3f36b221`. Not in
  the repository.
- **Licence:** OFL-1.1.
- **Read at:** `fonts/LICENSE` at that commit,
  <https://raw.githubusercontent.com/notofonts/notofonts.github.io/025970232f4f8ff349310d9785431e87d20ed27c/fonts/LICENSE>
  (`[[source: notofonts-fonts-license]]`). Read 2026-11-07. The repository's
  git tree at that commit, listed whole through the GitHub API, has no
  licence file anywhere under the font's own directory.
- **What it says:** "Copyright 2022 The Noto Project Authors
  (https://github.com/notofonts/)", then the OFL 1.1 with the same grant and
  condition 5 as above. The README at the same commit
  (`[[source: notofonts-readme]]`) says: "All Noto fonts (in the `fonts/`
  directory) are licensed under the SIL Open Font License. This
  documentation and all tooling in this repository is licensed under the
  Apache 2.0 License." The root `LICENSE` is that Apache License 2.0, and it
  does not cover the fonts.
- **Kept copy:** `docs/research/sources/notofonts-fonts-LICENSE-0259702.txt`.
- **YouTube:** allowed. No NonCommercial or NoDerivatives term, and the
  video does not carry the font file.

## Noto Sans Devanagari

- **Piece:** the Hindi line, as a face the demo fetches when it runs, from
  <https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf>,
  sha256 `9c7d935139ea6a1e6ad9dbac4f6d27ece1e04bca8123c8888d00a0f9df4724cd`.
  Not in the repository.
- **Licence, page, date, quotation and kept copy:** the same as Noto Naskh
  Arabic's. It is in the same `fonts/` directory, under the same
  `fonts/LICENSE`, read 2026-11-07, kept at
  `docs/research/sources/notofonts-fonts-LICENSE-0259702.txt`.
- **YouTube:** allowed, for the same reasons.

## Noto Sans Symbols 2

- **Piece:** btop's braille. Fetched when the demo runs, from
  <https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansSymbols2/unhinted/ttf/NotoSansSymbols2-Regular.ttf>,
  sha256 `c4a0a80f0041ce4be81e2478faad22776d23edb98ae3f0d19bd37044820ecf9d`.
  Not in the repository.
- **Licence, page, date, quotation and kept copy:** the same as Noto Naskh
  Arabic's: OFL-1.1, `fonts/LICENSE` at commit 0259702, read 2026-11-07,
  kept at `docs/research/sources/notofonts-fonts-LICENSE-0259702.txt`.
- **YouTube:** allowed, for the same reasons.

## The fonts embedded in the app

These are in the repository already, each with its licence beside it and its
row in `THIRD_PARTY.md`. They were not fetched again for this file. The
notices below were read in the tree on 2026-11-07.

### JetBrains Mono

- **Licence:** OFL-1.1.
- **Notice:** `baud-app/fonts/jetbrains-mono/OFL.txt`, which begins
  "Copyright 2020 The JetBrains Mono Project Authors
  (https://github.com/JetBrains/JetBrainsMono)".
- **Upstream:** <https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304>.
- **YouTube:** allowed. From "PERMISSION & CONDITIONS" to "TERMINATION",
  this notice's text is word for word the text quoted above for Noto Color
  Emoji, condition 5 included, and it has no NonCommercial or NoDerivatives
  term. The same holds for the two notices below.

### baud's embedded font

- **Piece:** the text face in four styles: JetBrains Mono with Hebrew drawn
  from Cascadia Mono and with Noto Sans Devanagari 2.007 merged in, a
  Modified Version under the OFL.
- **Licence:** OFL-1.1.
- **Notice:** `baud-app/fonts/built/OFL.txt`, which carries three copyright
  lines: The JetBrains Mono Project Authors; "Microsoft Corporation, with
  Reserved Font Name Cascadia Code"; "The Noto Project Authors
  (https://github.com/notofonts/devanagari)".
- **Upstream:** <https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304>,
  <https://github.com/microsoft/cascadia-code/releases/tag/v2407.24> and
  <https://github.com/notofonts/devanagari>.
- **YouTube:** allowed, as for JetBrains Mono.

### STIX Two Math

- **Licence:** OFL-1.1.
- **Notice:** `baud-app/fonts/stix-two-math/OFL.txt`, which begins
  "Copyright 2001-2021 The STIX Fonts Project Authors
  (https://github.com/stipub/stixfonts)".
- **Upstream:** <https://github.com/stipub/stixfonts/tree/v2.13b171>.
- **YouTube:** allowed, as for JetBrains Mono.

### Symbols Nerd Font Mono

- **Licence:** MIT AND OFL-1.1 AND Apache-2.0 AND CC-BY-4.0 AND Unlicense.
  The font is MIT; each icon set keeps its own licence, as
  `baud-app/fonts/symbols-nerd-font/README.md` lists and
  `baud-app/fonts/symbols-nerd-font/icon-licences/` holds.
- **Notice:** `baud-app/fonts/symbols-nerd-font/LICENSE`, "Copyright (c) 2014
  Ryan L McIntyre".
- **Upstream:** <https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1>.
- **Attribution:** Codicons is under CC BY 4.0. Its section 3(a)(1) says
  that if you share the material you must retain, among other things,
  "identification of the creator(s)", "a copyright notice" and "a notice
  that refers to this Public License" where the licensor supplies them, and
  3(a)(1)(B) that you indicate if you modified it. Its licence file in the
  tree names no creator. Its package names its author: "Microsoft
  Corporation" (`package.json` of microsoft/vscode-codicons, read on
  2026-11-07 at
  <https://raw.githubusercontent.com/microsoft/vscode-codicons/main/package.json>).
  Nerd Fonts patches the icons into Symbols Nerd Font Mono, so the credit
  line below names Codicons, Microsoft Corporation, the repository, the
  licence, and "as modified by Nerd Fonts". No scene is known to draw one;
  the credit stands in case one does.
- **YouTube:** allowed. None of these licences has a NonCommercial or a
  NoDerivatives term.

## nixos.org, shown in Chawan

- **Piece:** the page at <https://nixos.org>, which the tour's scene 9
  opens from the README's Nix link and Chawan draws, its pictures on. It is
  fetched live when the tour is filmed and is not in the repository.
- **Licence:** CC BY-SA 4.0, for the site's content. The page's footer says
  "Copyright © 2026 NixOS contributors" and links its licence, "CC-BY-SA-4.0",
  to <https://github.com/NixOS/nixos-homepage/blob/main/LICENSES/CC-BY-SA-4.0.txt>.
  Read 2026-11-08.
- **The NixOS logo on it:** "Logo originally designed by Simon Frankau
  (@simon-frankau) and revised by Tim Cuthbertson (@timbertson)", under a
  CC-BY licence linked to CC BY 4.0, as the logo's README says,
  <https://raw.githubusercontent.com/NixOS/nixos-artwork/master/logo/README.md>.
  Read 2026-11-08.
- **YouTube:** allowed with the credit below. Neither licence has a
  NonCommercial or a NoDerivatives term; each asks for attribution and a
  link to the licence, and CC BY-SA asks that adaptations of the material
  carry the same licence. The video shows the page as Chawan draws it.

## Chawan

- **Piece:** the text-mode browser that draws nixos.org in scene 9, run on
  the Mac from nixpkgs (`chawan` 0.4.4, in `PROGRAMS` in
  `baud-app/examples/demo/run.rs`). Not in the repository.
- **Licence:** the Unlicense. Its `UNLICENSE` at 0.4.4, in nixpkgs' source
  of it, begins "This is free and unencumbered software released into the
  public domain."; its README says "Chawan is dedicated to the public
  domain. See the UNLICENSE file for details." nixpkgs gives its licence as
  `Unlicense`. Read 2026-11-08.
- **YouTube:** allowed. The Unlicense puts no condition on use.

## baud's own pieces

- The plasma logo film, `assets/readme/logo-dark.gif`. Its letters are
  JetBrains Mono's glyphs, under the OFL, as `THIRD_PARTY.md`'s row for the
  README's logo says.
- The UFO logo film, `assets/demo/ufo-logo-dark.gif` and
  `ufo-logo-light.gif`, made by `baud-pictures logo --ufo`: the ufo trail's
  saucer threading the logo's letters. Its letters are JetBrains Mono's
  glyphs, under the OFL, in the same `THIRD_PARTY.md` row as the README's
  logo.
- The tour itself, which the trailer plays from YouTube through mpv and
  yt-dlp.

These are the project's own work, under the repository's licence
(`LICENSE`).

## hello.txt

`hello.txt` (`HELLO` in `baud-app/examples/demo/session.rs`) is baud's own
text: the word for baud in Hebrew, Arabic, Chinese, Japanese, Korean and
Hindi, each followed by a one-line definition written for baud, and a line
of emoji. Nothing in it is quoted. The dictionaries, standards and glossary
named in `session.rs` were read only to get the meaning and the words for
baud right; `MANIFEST.toml` records each as `external`, read and not kept.
It is the project's own work, under the repository's licence (`LICENSE`).

## Music and sound effects

The tour has no music. Its sound effects are not chosen yet, and no audio is
in the repository. This is a slot: a sound chosen later, such as the key
sounds of a real keyboard's recording, gets its own section here, read and
kept the same way.

## The credit line for the video's description

```text
Fonts: JetBrains Mono, by The JetBrains Mono Project Authors (SIL OFL 1.1). baud's text face is JetBrains Mono with Hebrew from Cascadia Mono by Microsoft Corporation and with Noto Sans Devanagari by The Noto Project Authors (SIL OFL 1.1). STIX Two Math, by The STIX Fonts Project Authors (SIL OFL 1.1). Symbols Nerd Font Mono, by Ryan L McIntyre (Nerd Fonts, MIT), with each icon set under its own licence, among them Codicons by Microsoft Corporation (github.com/microsoft/vscode-codicons), as modified by Nerd Fonts, CC BY 4.0, creativecommons.org/licenses/by/4.0/. Noto Color Emoji, Copyright 2013 Google LLC (SIL OFL 1.1). Noto Sans CJK SC, by Adobe and Google (SIL OFL 1.1). Noto Naskh Arabic, Noto Sans Devanagari and Noto Sans Symbols 2, by The Noto Project Authors (SIL OFL 1.1).
nixos.org shown in Chawan: content © 2026 NixOS contributors, CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/).
NixOS logo by Simon Frankau, revised by Tim Cuthbertson, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).
Chawan: Unlicense.
```
