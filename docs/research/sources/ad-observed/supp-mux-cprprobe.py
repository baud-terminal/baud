# Inside a multiplexer pane: width of each cluster as the multiplexer's own cursor reports it (CPR).
import os, sys, termios, tty, time, json, select, re
cases=[]
for l in open(sys.argv[1]):
    f=l.split(); cases.append((f[0],[int(h,16) for h in f[1:]]))
fd=os.open('/dev/tty',os.O_RDWR); old=termios.tcgetattr(fd)
def w(s): os.write(fd,s.encode())
def cpr():
    w('\x1b[6n'); buf=b''; t0=time.time()
    while time.time()-t0<3:
        r,_,_=select.select([fd],[],[],0.2)
        if r:
            buf+=os.read(fd,64); m=re.search(rb'\x1b\[(\d+);(\d+)R',buf)
            if m: return int(m.group(1)),int(m.group(2))
    return None
tty.setraw(fd); res={}
try:
    for cid,cps in cases:
        w('\r\n\x1b[5C'); a=cpr()
        w(''.join(map(chr,cps))); b=cpr()
        res[cid]={'before':a,'after':b,'width':(b[1]-a[1]) if a and b else None}
finally:
    termios.tcsetattr(fd,termios.TCSADRAIN,old)
json.dump(res,open(sys.argv[2],'w'),indent=1)
w('\r\nCPRDONE\r\n'); time.sleep(1)
