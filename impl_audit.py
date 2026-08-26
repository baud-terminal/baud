import re, sys
src=open(sys.argv[1]).read()
prim=r'\b(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b'

cats = {
 'raw index arithmetic  (y * MAX_COLS + x)': r'\*\s*MAX_COLS',
 'usize params on operations':               r'fn \w+\(&mut self[^)]*: usize',
 'usize/u-typed struct fields':              r'^\s+\w+: (usize|u8|u16|u32|u64),',
 'raw casts (as usize / as u16 / as u32)':   r'\bas (usize|u16|u32|u8)\b',
 'raw byte range tests':                     r'0x[0-9a-f]+\.\.0x[0-9a-f]+\)\.contains',
 'bool as a flag field':                     r'^\s+\w+: bool,',
}
print(f"  {'category':<44} {'occurrences'}")
tot=0
for name, pat in cats.items():
    n=len(re.findall(pat, src, re.M))
    tot+=n
    print(f"  {name:<44} {n}")
print(f"  {'':<44} ----")
print(f"  {'total':<44} {tot}")
