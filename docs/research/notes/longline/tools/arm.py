import sys
root, arm = sys.argv[1], sys.argv[2]
p = root + "/baud-domain/src/grid.rs"
s = open(p).read()
held = "                PastEdge::Held => reach_of_row(self.cells, self.used, s, p, w, end),\n"
clear = "                PastEdge::Clear if w < was => match self.cells[p * s + was - 1] {\n"
assert s.count(held) == 1 and s.count(clear) == 1
if arm == "held2":
    s = s.replace(held, "                PastEdge::Held => {\n                    core::hint::black_box(reach_of_row(core::hint::black_box(self.cells), self.used, s, p, w, end));\n                    reach_of_row(self.cells, self.used, s, p, w, end)\n                }\n")
elif arm == "clear2":
    s = s.replace(clear, "                PastEdge::Clear if w < was => match {\n                    core::hint::black_box(core::hint::black_box(&*self.cells)[p * s + was - 1]);\n                    self.cells[p * s + was - 1]\n                } {\n")
else:
    assert arm in ("base", "aa")
open(p, "w").write(s)
