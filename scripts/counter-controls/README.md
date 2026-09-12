# Identifying `xctrace`'s counter columns

`xctrace record --template 'CPU Counters'` works where `dtrace`'s `cpc`
provider is SIP-blocked, and its export carries **twelve unnamed integers** per
sample. Nothing in the table export says which is which, and this repo has
twice claimed an identification from magnitudes alone and twice withdrawn it —
both times because the ratio agreed with what was already believed.

These are the controls that identify them, and each is a program whose
behaviour is known by construction rather than inferred.

**Invoke the real `xctrace`, not the `/usr/bin` shim.** `/usr/bin/xctrace` is a
`xcode-select` stub that forwards to the active developer directory, and a
nix-provided toolchain sets `DEVELOPER_DIR` to an `apple-sdk-*` that carries no
Instruments -- so the shim answers "tool 'xctrace' not found" and the whole
procedure looks blocked when it is not. Use the real tool:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
XC="$DEVELOPER_DIR/usr/bin/xctrace"   # Xcode 16.0; records fine
```

and run `$XC record ...` in place of `xcrun xctrace record` below. Three
"not found" errors that all inherit one bad `DEVELOPER_DIR` are one probe that
cannot fire, not three that agree.

```sh
rustc -O -o chase chase.rs && rustc -O -o branch branch.rs

xcrun xctrace record --template 'CPU Counters' --output l1.trace   --launch -- ./chase 32 400
xcrun xctrace record --template 'CPU Counters' --output dram.trace --launch -- ./chase 65536 20
xcrun xctrace record --template 'CPU Counters' --output bp.trace   --launch -- ./branch pred 600
xcrun xctrace record --template 'CPU Counters' --output br.trace   --launch -- ./branch rand 300

for t in l1 dram bp br; do
  xcrun xctrace export --input $t.trace \
    --xpath '/trace-toc/run[@number="1"]/data/table[@schema="kdebug-counters-with-time-sample"]' > $t.xml
  python3 pmc.py $t.xml
done
```

`pmc.py` aggregates **consecutive same-core** deltas only. The counters are
per-core and cumulative, so a thread migrating produces a delta that is not
work: taking max-minus-min across all samples implies an 8.34 GHz clock, and
this rule gives the machine's actual 4.5.

`chase` is a pointer chase over a randomised permutation: exactly one dependent
load a step, and one cache miss a step when the buffer is far larger than any
cache, none when it is 32 KB. `branch` is a loop whose branch is either
alternating or a coin flip, with the same instruction count either way — its
first version compiled to a conditional select and both arms measured the
same, which is a probe that cannot fire, so the two arms are `#[inline(never)]`
calls.

**The metric names are in `xctrace export --toc`**, not in the table export:
the `CoreTypeByProcess` table's `swift-table` attribute carries a
`metricLegend` naming Cycles, Instruction Delivery Bottleneck, Discarded
Bottleneck, Instruction Processing Bottleneck and Useful. It does not say which
integer column is which, which is what these controls are for.

`CLAUDE.md` carries the identifications and the swings they rest on.
