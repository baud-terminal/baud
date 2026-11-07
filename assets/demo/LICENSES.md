# The films' pieces and their licences

baud's demo films two things: the tour, an MP4 for YouTube, and the trailer,
the README's film, a GIF of the tour playing. This file lists every piece the
tour shows and the licence each comes under. No trailer has been filmed yet,
so none is in the repository; its row is added here once its take exists.

Each third-party licence below was read on 2026-11-07 at the URL given. For
the photo and the fonts the demo fetches, a copy of each licence file read is
kept in `docs/research/sources/`, with its entry in
`docs/research/sources/MANIFEST.toml` (group FM) and its row in
`THIRD_PARTY.md`. A source marked `external` there was read and hashed but is
not kept. The fonts embedded in the app keep their notices beside them under
`baud-app/fonts/`.

## The photo: "Canazei Granite Ridges"

- **Piece:** the elementary OS wallpaper "Canazei Granite Ridges", which
  kitten draws in the picture shot. The demo fetches it when it runs, from
  <https://raw.githubusercontent.com/elementary/wallpapers/8.0.0/backgrounds/Canazei%20Granite%20Ridges.jpg>,
  and checks its sha256,
  `0eff9cd7fe92aabb8937644ca154436ba926310808dd99a2902cdde335b0868c`. The
  file fetched on 2026-11-07 had that hash. It is not in the repository.
- **Licence:** CC0-1.0.
- **Read at:** elementary's `debian/copyright`, on the deb-packaging branch
  at its head that day, commit ba8b274 ("Release 8.0.0"):
  <https://raw.githubusercontent.com/elementary/wallpapers/ba8b274cbb0afeda198a62007480ce89d00052f1/debian/copyright>
  (`[[source: elementary-wallpapers-debian-copyright]]`). Read 2026-11-07.
- **What it says:** "Unsplash photos published before June 8, 2017 are CC0
  (public domain):". The list under that sentence holds "Canazei Granite
  Ridges <https://unsplash.com/photos/yrwpJwDNSHE>".
- **CC0's legal code:** read at
  <https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt>
  (`[[source: cc0-1-0-legalcode]]`), 2026-11-07. Section 2 waives the rights
  "in any current or future medium and for any number of copies, and (iv) for
  any purpose whatsoever, including without limitation commercial,
  advertising or promotional purposes".
- **Kept copy:** `docs/research/sources/cc0-1.0-legalcode.txt`. Creative
  Commons makes its legal code available under CC0
  (`[[source: cc-policies]]`).
- **debian/copyright, read and not kept:** the repository's `LICENSE.md`
  at tag 8.0.0 says only "For individual wallpaper licensing information,
  see:" and links this file (`[[source: elementary-wallpapers-license-md]]`).
  No licence is named for the file itself, so a copy here would be
  redistribution with no grant. `MANIFEST.toml` records it as `external`:
  its URL at ba8b274 and the SHA-256 of the copy read.
- **The Unsplash page:** <https://unsplash.com/photos/yrwpJwDNSHE> answered
  HTTP 401 on 2026-11-07, with a page titled "Checking your browser before
  accessing unsplash.com". It could not be read as text. So the photographer
  and Unsplash's own date for the photo are not confirmed here, and
  `debian/copyright` names no photographer.
- **YouTube:** allowed, resting on elementary's statement in
  `debian/copyright` that the photo is CC0. The Unsplash page could not be
  read to confirm it, as below. CC0 has no NonCommercial term and no
  NoDerivatives term, and section 2 waives the rights for any purpose,
  commercial included.

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

## baud's own pieces

- The plasma logo film, `assets/readme/logo-dark.gif`, and the MP4 that
  `baud-pictures logo --mp4` makes from it, which mpv plays in the tour. Its
  letters are JetBrains Mono's glyphs, under the OFL, as `THIRD_PARTY.md`'s
  row for the README's logo says.
- The UFO logo film, `assets/demo/ufo-logo-dark.gif` and
  `ufo-logo-light.gif`, made by `baud-pictures logo --ufo`, which kitten
  draws in the tour's scene 9: the ufo trail's saucer threading the logo's
  letters. Its letters are JetBrains Mono's glyphs, under the
  OFL, in the same `THIRD_PARTY.md` row as the README's logo.
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
Photo: "Canazei Granite Ridges", an elementary OS wallpaper from Unsplash (unsplash.com/photos/yrwpJwDNSHE), CC0 1.0.
Fonts: JetBrains Mono, by The JetBrains Mono Project Authors (SIL OFL 1.1). baud's text face is JetBrains Mono with Hebrew from Cascadia Mono by Microsoft Corporation and with Noto Sans Devanagari by The Noto Project Authors (SIL OFL 1.1). STIX Two Math, by The STIX Fonts Project Authors (SIL OFL 1.1). Symbols Nerd Font Mono, by Ryan L McIntyre (Nerd Fonts, MIT), with each icon set under its own licence, among them Codicons by Microsoft Corporation (github.com/microsoft/vscode-codicons), as modified by Nerd Fonts, CC BY 4.0, creativecommons.org/licenses/by/4.0/. Noto Color Emoji, Copyright 2013 Google LLC (SIL OFL 1.1). Noto Sans CJK SC, by Adobe and Google (SIL OFL 1.1). Noto Naskh Arabic, Noto Sans Devanagari and Noto Sans Symbols 2, by The Noto Project Authors (SIL OFL 1.1).
```
