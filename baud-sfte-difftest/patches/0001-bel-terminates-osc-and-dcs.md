# 0001: BEL terminates an OSC or DCS string

**What upstream does.** sfte's per-byte parser entry opens with a C0 switch that
runs before the state switch, and its `'\a'` arm rings the bell and `return`s
in every parser state. So the `b == '\x07'` terminator arms inside `VT_OSC`
and `VT_DCS` are unreachable, and `ESC ] 0 ; title BEL` -- the commonest
window-title spelling in the wild -- leaves the parser in `VT_OSC`, where it
swallows every byte until the next `ESC` arrives and then dispatches the OSC
with all of them as its body. Reproduced on the pristine header: after
`ESC ] 0 ; title BEL AFTER` the screen is blank and the cursor is at (0, 0).

**What the patch does.** One condition in front of the bell: in `VT_OSC` or
`VT_DCS` the byte falls through to the state switch, where the existing
terminator arm handles it. Nothing else changes. Patched: `AFTER` is on row 0
and the cursor is at (0, 5).

**Why patch rather than pin.** The owner's rule is that no recording is
excluded from the differential, and a BEL-terminated OSC is the commonest
window-title spelling in the wild, so an oracle that wedges on it is an
oracle that cannot be trusted on any session that sets a title. A one-line
spec conformance fix leaves sfte an independent implementation.

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
found zero or several times. A patch that silently fails to apply is the
no-op edit `CLAUDE.md` records three times, and the build refusing is the
positive control. The altered copy carries a comment naming this file, which
is what the zlib licence's second clause asks for.

**Upstream.** Worth a courteous report; not sent yet.

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
        /* baud patch 0001 (baud-sfte-difftest/patches/): BEL terminates an
           OSC or DCS string, so in those states it must reach the state
           switch below rather than ring the bell and return. Upstream as
           shipped returns here in every state, which makes the terminator
           arms in VT_OSC and VT_DCS unreachable. */
        if (ctx->term.parser_state == VT_OSC || ctx->term.parser_state == VT_DCS) break;
        if (ctx->bell_cb) ctx->bell_cb(ctx->user_data);
        return;
    default: break;
    }

    switch (ctx->term.parser_state) {
```
