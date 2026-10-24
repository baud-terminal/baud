import sys, zlib, struct, numpy as np
def load(p):
    b = open(p, 'rb').read(); i = 8; data = b''
    while i < len(b):
        n, t = struct.unpack('>I4s', b[i:i+8]); d = b[i+8:i+8+n]
        if t == b'IHDR': w, h = struct.unpack('>II', d[:8])
        if t == b'IDAT': data += d
        i += 12 + n
    raw = zlib.decompress(data); a = np.frombuffer(raw, np.uint8).reshape(h, 1 + 3 * w)[:, 1:].reshape(h, w, 3)
    return a.astype(int)
a, b = load(sys.argv[1]), load(sys.argv[2]); skip = a.shape[0] // 5
d = np.abs(a[skip:] - b[skip:]).max(axis=2)
print(sys.argv[2].split('/')[-1], 'pixels differing', int((d > 0).sum()), 'max channel delta', int(d.max()))
