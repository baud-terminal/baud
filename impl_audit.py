import re, sys
src=open(sys.argv[1]).read()
prim=r'\b(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b'

cats = {
 'raw index arithmetic  (y * MAX_COLS + x)': r'\*\s*MAX_COLS',
 'usize params on operations':               r'fn \w+\(&mut self[^)]*: usize',
 # The rule above only saw `usize`, and only on `&mut self` methods. It read 0
 # for a file with eight raw-primitive sites -- `final_byte: u8`, `param: u8`,
 # `-> Option<u8>` and an `app: bool` that the bool rule below could not see
 # either, because that one is anchored to a struct field. A gate that agrees
 # with whoever wrote the code is the failure this repo keeps recording.
 'raw primitive fn params':                  r'fn \w+\([^)]*?\b\w+: (u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b',
 'raw primitive fn returns':                 r'fn \w+\([^)]*\)\s*->\s*(?:Option<)?(u8|u16|u32|u64|usize)\b',
 'bool as a fn param':                       r'fn \w+\([^)]*?\b\w+: bool\b',
 'usize/u-typed struct fields':              r'^\s+\w+: (usize|u8|u16|u32|u64),',
 'raw casts (as usize / as u16 / as u32)':   r'\bas (usize|u16|u32|u8)\b',
 'raw byte range tests':                     r'0x[0-9a-f]+\.\.0x[0-9a-f]+\)\.contains',
 'bool as a flag field':                     r'^\s+\w+: bool,',
}
# The signature categories are reported and gated SEPARATELY from the original
# ones. Folding them into one number would either fail a limit calibrated to a
# narrower detector, or -- worse -- invite raising that limit, which this
# repo's notes forbid. Two numbers, each pinned at what it actually is.
SIGNATURE = {
    'raw primitive fn params',
    'raw primitive fn returns',
    'bool as a fn param',
}
print(f"  {'category':<44} {'occurrences'}")
tot = 0
sig = 0
for name, pat in cats.items():
    n = len(re.findall(pat, src, re.M))
    if name in SIGNATURE:
        sig += n
    else:
        tot += n
    print(f"  {name:<44} {n}")
print(f"  {'':<44} ----")
print(f"  {'signature':<44} {sig}")
print(f"  {'total':<44} {tot}")
