import sys
sys.path.insert(0, "/nix/store/4fdyl65zzmwlzaz1ymin1756jrzjm9i2-python3.14-uharfbuzz-0.53.2/lib/python3.14/site-packages")
import uharfbuzz as hb
FEAT = {f: False for f in "abvm blwm locl rlig clig curs dist kern liga rclt rvrn dlig hlig".split()}
faces = {}
def shape(path, idx, cps, script=None):
    if (path, idx) not in faces:
        blob = hb.Blob.from_file_path(path); faces[(path, idx)] = hb.Font(hb.Face(blob, idx))
    buf = hb.Buffer(); buf.add_codepoints(cps); buf.guess_segment_properties()
    if script: buf.script = script
    hb.shape(faces[(path, idx)], buf, FEAT)
    return tuple((i.codepoint, i.cluster, p.x_offset, p.y_offset, p.x_advance) for i, p in zip(buf.glyph_infos, buf.glyph_positions))
