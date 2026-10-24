import sys, struct, zlib, glob, os
def load(p):
    b=open(p,'rb').read(); w,h=struct.unpack('<II',b[:8]); px=b[8:]
    rows=[]
    for y in range(h):
        r=bytearray()
        for x in range(w):
            v=struct.unpack_from('<I',px,(y*w+x)*4)[0]
            r+=bytes(((v>>16)&255,(v>>8)&255,v&255))
        rows.append(bytes(r))
    return w,h,rows
def png(path,w,h,rows):
    raw=b''.join(b'\x00'+r for r in rows)
    def chunk(t,d): return struct.pack('>I',len(d))+t+d+struct.pack('>I',zlib.crc32(t+d)&0xffffffff)
    open(path,'wb').write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw,9))+chunk(b'IEND',b''))
d=sys.argv[1]; allrows=[]; W=None
for p in sorted(glob.glob(d+'/*.rgba')):
    w,h,rows=load(p); png(p[:-5]+'.png',w,h,rows); W=w
    allrows+=rows+[bytes((90,90,90))*w]*3
    os.remove(p)
png(d+'/sheet-all-options.png',W,len(allrows),allrows)
