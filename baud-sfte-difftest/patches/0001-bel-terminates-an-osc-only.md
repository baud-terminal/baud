# 0001: BEL terminates an OSC, and no other string

**What upstream does.** sfte's per-byte parser entry opens with a C0 switch that
runs before the state switch, and its `'\a'` arm (sfte.h 34974-34976) rings the
bell and `return`s in every parser state. So the `b == '\x07'` terminator arm
inside `VT_OSC` (35042-35043) is unreachable, and `ESC ] 0 ; title BEL` -- the
commonest window-title spelling in the wild -- leaves the parser in `VT_OSC`,
where it swallows every byte until the next `ESC` arrives and then dispatches
the OSC with all of them as its body. Reproduced on the pristine header: after
`ESC ] 0 ; title BEL AFTER` the screen is blank and the cursor is at (0, 0).

**What the texts say.** `[[source: ctlseqs]]`: "XTerm accepts either BEL or ST
for terminating OSC sequences" (2023-2024), and it gives no other control
string a BEL terminator. `[[source: williams]]` gives BEL, with the rest of
`00-17`, the *put* action in `dcs passthrough` (388) and *ignore* in
`sos/pm/apc string` (426): in neither does it end the string, and since put
and ignore execute nothing, in neither does it ring.

**What the patch does.** One condition in front of the bell: in `VT_OSC` the
byte falls through to the state switch, where the existing terminator arm
handles it. Nothing else changes. Patched: `AFTER` is on row 0 and the cursor
is at (0, 5).

**What it leaves alone, and why it was narrowed.** sfte enters one state,
`VT_DCS`, on `ESC P`, `ESC _` and `ESC ^` (34615, 35012-35013) -- a DCS, an
APC and a PM -- and that arm carries the same `b == '\x07'` terminator test
(35049-35050), which the early return makes unreachable too. This patch's
first version, `0001-bel-terminates-osc-and-dcs.md`, let the BEL through in
`VT_DCS` as well and called the whole "a one-line spec conformance fix". For
those three strings it was the opposite: a departure from both texts, patched
into the oracle. baud-core made the same departure until 72e5ce0, so the two
agreed in it and nothing could show it. Since 72e5ce0 baud ends only an OSC on
a BEL, and four fixtures in `main.rs` tell the two conditions apart. Under the
old one sfte ended the string at the BEL and drew the `X` after it: `BEL in a
DCS body`, `BEL in an APC` and `BEL in a PM` each diverged in six cells and the
cursor, and `BEL in DECRQSS`, pinned on its reply alone, went off its pin in
the same six cells. Under this one all four are where they belong, and every
recording is on its pin unchanged.

**What stays unpatched.** Inside a DCS, an APC or a PM, sfte still rings the
bell for a BEL and keeps the byte out of the string's payload, where williams
rings in none of the three and puts the byte in a DCS's. The differential sees
neither: it compares no bell, and the one payload sfte reads in that state is
one that starts `+q`, whichever of the three introduced it (`SFTE_IMG_KITTY` and
`SFTE_IMG_SIXEL` are 0 in `sfte_shim.c`, 34-35, which compiles out its two other
readers, the kitty `G` dispatch at sfte.h 34944-34946 and the Sixel header test
at 35054-35075), and baud answers no
`+q`, so no fixture sends one. Left as found, sfte's departure.

**Why patch rather than pin.** No recording is excluded from the differential, and a BEL-terminated OSC is the commonest
window-title spelling in the wild, so an oracle that wedges on it is an oracle
that cannot be trusted on any session that sets a title. One condition, scoped
to the one string the texts let a BEL end, leaves sfte an independent
implementation.

**What the corpus says, measured rather than assumed.** `cava.raw` opens with
one BEL-terminated OSC, and with the patch removed it *still* agrees at all
156 boundaries -- the bytes swallowed before the next `ESC` are the title's
and nothing that lands on screen. So the corpus could not have shown this
defect; the `BEL-terminated OSC` fixture in `main.rs` is what does (five
cells and the cursor diverge with the patch disabled), and it is the live
control that says the patch is applied.

**How it is applied.** `build.rs` copies `docs/research/sources/sfte.h` --
which stays pristine and hashed in `MANIFEST.toml` -- into `OUT_DIR` and
replaces the anchor below **exactly once**, failing the build if the anchor is
found zero or several times. A patch that silently fails to apply is an edit that changed nothing and said so nowhere (CONTRIBUTING.md, "Refactoring failures that already happened here"), and the build refusing is the
positive control. The altered copy carries a comment naming this file, which
is what the zlib licence's second clause asks for.

**Upstream.** Worth a courteous report, for the OSC alone; not sent yet.

## anchor

```c
    case '\a':
        if (ctx->bell_cb) ctx->bell_cb(ctx->user_data);
        return;
    default: break;
    }

    switch (ctx->term.parser_state) {
```

## replacement

```c
    case '\a':
        /* baud patch 0001 (baud-sfte-difftest/patches/0001-bel-terminates-an-osc-only.md):
           BEL terminates an OSC, so in VT_OSC it must reach the state switch
           below rather than ring the bell and return. Upstream as shipped
           returns here in every state, which makes VT_OSC's BEL terminator
           arm unreachable. VT_DCS, which is ESC P, ESC _ and ESC ^ here, is
           left as shipped: a BEL ends no DCS, APC or PM (ctlseqs 2023-2024;
           williams 388 and 426). */
        if (ctx->term.parser_state == VT_OSC) break;
        if (ctx->bell_cb) ctx->bell_cb(ctx->user_data);
        return;
    default: break;
    }

    switch (ctx->term.parser_state) {
```
