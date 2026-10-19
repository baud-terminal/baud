# vttest's double-size screens

What vttest sends a terminal for its double-width and double-height tests,
captured through a real pty rather than written by hand. vttest is Thomas
Dickey's VT100 test program; these were taken from the snapshot at commit
`0229d7171a8574a2bf406c6ce14549f65d810e51` (version 2.7, patch 20251205),
built out of tree with its own `configure` and `make`, and driven by its menu
keys:

```sh
python3 scripts/capture-tui.py --seconds 8 --rows 24 --cols 80 \
  --out tests/vttest/doublesize.raw --first-key 1.0 --key-delay 0.6 \
  --keys '4\r\r\r\r\r\r\r\r0\r' -- ./vttest 24x80.132
```

| file | menu keys | what it draws |
|---|---|---|
| `doublesize.raw` | `4` | "Test of double-sized characters": each rendition at 80 and 132 columns, a DECSWL and a DECDWL of the same line, the frame of DEC line drawing on double-height lines, and twelve RIs that leave half of it |
| `bug-b.raw` | `9`, `2` | "Bug B: Scrolling region": a double-width line above a region, and RI and sixteen lines scrolled under it |
| `bug-e.raw` | `9`, `5` | "Bug E: Cursor move from double- to single-wide line", at 132 columns |
| `bug-l.raw` | `9`, `8` | "Erase right half of double width lines", at 80 and 132 columns |

The pty echoes the menu keys, so they are in the streams as vttest's
terminal received them. vttest asks for a primary DA at start and the capture
answered nothing; the tests it runs here read no reply.

`baud-core/tests/vttest_double_size.rs` replays each through a terminal that
honours `?3`'s width request and checks the screen at every "Push <RETURN>":
the line renditions, the text and the cursor, against DEC STD 070 rather than
against vttest's prose. The two differ at Bug L, whose prose calls it a
misfeature that the right half of a long line does not come back after
DECDWL and DECSWL; DEC STD 070, the VT100 User Guide and the VT510 manual say
it is lost, and the test holds the text.
