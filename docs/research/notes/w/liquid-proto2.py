# Prototype v2 of the liquid trail (design (a)), after review:
#  - neck, pinch, beads and light on the IGNITION clock (survive a retarget);
#  - tube radius eased from its value at the retarget, and ramped to zero with
#    the spine length (no pop, no notch);
#  - head drop: round = gap_max + 1 px first, half_box >= round per axis;
#  - wobble: a sum over the last two contacts, so a retarget drops none.
# Renders dark and light themes, a 30-col hop, a diagonal, and a retarget, and
# prints parameter jumps across 1 ms either side of the retarget.
import math
from liquid import CW, CH, pcg, unit, cell, corners, seg_new, seg_at, smoothstep, smin

G=0.10; AMAX=0.08; K=0.15; BEZ=0.25; NECK=1.45
A_BODY,A_RIM,S_MAIN,S_BACK=0.12,0.55,0.85,0.25
ZETA, FREQ = 0.28, 6.5
FADE_IN, FADE_OUT = 0.035, 0.10
TAU_R = 0.05
MIN_END = 0.22
OM = 2*math.pi*FREQ; OD = OM*math.sqrt(1-ZETA*ZETA)

def axis(cs, hole):
    H=((hole[0]+hole[2])/2,(hole[1]+hole[3])/2)
    m=(sum(c[0] for c in cs)/4, sum(c[1] for c in cs)/4)
    vx,vy=H[0]-m[0],H[1]-m[1]; L=math.hypot(vx,vy)
    if L<1e-3: return H,H,None
    d=(vx/L,vy/L)
    pr=sorted((c[0]-H[0])*d[0]+(c[1]-H[1])*d[1] for c in cs)
    sb=0.5*(pr[0]+pr[1])
    return (H[0]+d[0]*sb,H[1]+d[1]*sb),H,d

def radius_for(d):
    return 0.45*math.hypot(d[1]*CW, d[0]*CH)

def contact(seg, R):
    tgt=corners(seg[1]); tc=0.0
    for i in range(4):
        gap=max(abs(tgt[i][0]-seg[0][i][0]),abs(tgt[i][1]-seg[0][i][1]))
        if gap>R+G*CH: tc=max(tc, seg[2][i]/10*math.log2(gap/(R+G*CH)))
    return tc

class Ignition:
    def __init__(self, seg, seed):
        self.seed=seed
        B0,H0,d0=axis(seg_at(seg,0),seg[1])
        self.dir=d0; self.L0=math.hypot(H0[0]-B0[0],H0[1]-B0[1])
        self.neckmax=NECK*smoothstep(1.5,4.0,self.L0/CH)
        self.segs=[]   # (seg, start_age, R_start, R_new, tc, A)
        self.add(seg, 0.0, radius_for(d0))
    def add(self, seg, at, R_start):
        _,_,d=axis(seg_at(seg,0),seg[1]); Rn=radius_for(d) if d else R_start
        B0,H0,_=axis(seg_at(seg,0),seg[1]); L=math.hypot(H0[0]-B0[0],H0[1]-B0[1])
        A=AMAX*CH*smoothstep(3.0,12.0,L/CH)
        self.segs.append((seg,at,R_start,Rn,contact(seg,Rn),A))
        self.segs=self.segs[-2:]
        tail=[at+tc+(math.log(A/0.5)/(ZETA*OM) if A>0.5 else 0.0) for (_,at,_,_,tc,A) in self.segs]
        self.end=max(MIN_END, max(tail)+0.5*FADE_OUT)
    def radius(self, age):
        seg,at,Rs,Rn,_,_=self.segs[-1]; return Rn+(Rs-Rn)*math.exp(-(age-at)/TAU_R)
    def retarget(self, seg, age):
        self.add(seg, age, self.radius(age))

def model(ig, age):
    seg,at,_,_,_,_=ig.segs[-1]; since=age-at
    cs=seg_at(seg,since); hole=seg[1]
    B,H,d=axis(cs,hole)
    R=ig.radius(age)
    Lsp=math.hypot(H[0]-B[0],H[1]-B[1])
    Reff=R*smoothstep(0.0,R,Lsp)
    n=ig.neckmax*smoothstep(0.03,0.14,age)
    w=0.0
    for (_,sa,_,_,tc,A) in ig.segs:
        s=age-sa-tc
        if s>0: w+=A*math.exp(-ZETA*OM*s)*math.sin(OD*s)
    dd=ig.dir
    hx,hy=(hole[2]-hole[0])/2,(hole[3]-hole[1])/2
    gx=G*CH+w*abs(dd[0])-0.6*w*abs(dd[1]); gy=G*CH+w*abs(dd[1])-0.6*w*abs(dd[0])
    rho=G*CH+AMAX*CH+1.0
    bx=max(hx+gx,rho); by=max(hy+gy,rho)
    beads=[]
    if ig.neckmax>1.0 and Lsp>0:
        y=1.0/ig.neckmax; xp=0.5-math.sin(math.asin(1-2*y)/3); tp=0.03+0.11*xp
        cnt=2+(pcg(ig.seed^0x5bd1e995)&1)
        e=(H[0]-B[0],H[1]-B[1]); perp=(-dd[1],dd[0])
        for j in range(cnt):
            h0=pcg(ig.seed ^ ((j+1)*0x9E3779B9 & 0xffffffff)); h1=pcg(h0); h2=pcg(h1); h3=pcg(h2)
            sg=age-tp
            if sg<0: continue
            u0=0.5+(unit(h0)-0.5)*0.16; tj=0.045+0.04*unit(h2)
            u=u0+(1-u0)*(1-math.exp(-sg/tj))
            r=R*(0.30-0.07*j)*(0.85+0.3*unit(h1))*(1-smoothstep(0.85,0.99,u))*smoothstep(0,0.02,sg)
            o=(unit(h3)-0.5)*0.25*R*math.exp(-sg/0.06)
            if r>0: beads.append((B[0]+e[0]*u+perp[0]*o, B[1]+e[1]*u+perp[1]*o, r))
    sw=1.2*math.exp(-age/0.08); Lx,Ly=-0.5+sw*dd[0],-0.866+sw*dd[1]; ll=math.hypot(Lx,Ly)
    E=smoothstep(0,FADE_IN,age)*(1-smoothstep(ig.end-FADE_OUT,ig.end,age))
    return dict(B=B,H=H,R=Reff,n=n,box=(bx,by),rho=rho,K=K*CH,beads=beads,light=(Lx/ll,Ly/ll),E=E,hole=hole,w=w)

def field(p,M):
    H=M['H']; hx,hy=M['box']; rho=M['rho']
    px,py=p[0]-H[0],p[1]-H[1]
    sx=1.0 if px>=0 else -1.0; sy=1.0 if py>=0 else -1.0
    wx,wy=abs(px)-(hx-rho),abs(py)-(hy-rho); g=max(wx,wy)
    if g>0:
        ax,ay=max(wx,0),max(wy,0); lw=math.hypot(ax,ay); f=(lw-rho,sx*ax/lw,sy*ay/lw)
    else:
        f=(g-rho,)+((sx,0.0) if wx>wy else (0.0,sy))
    B=M['B']; e=(H[0]-B[0],H[1]-B[1]); L2=e[0]**2+e[1]**2
    if L2>0 and M['R']>0:
        u=min(max(((p[0]-B[0])*e[0]+(p[1]-B[1])*e[1])/L2,0),1)
        vx,vy=p[0]-(B[0]+e[0]*u),p[1]-(B[1]+e[1]*u); l=math.hypot(vx,vy)
        q=1-4*M['n']*u*(1-u); sl=4*M['n']*M['R']/math.sqrt(L2)*(2*u-1)
        dt=(l-M['R']*q)/math.sqrt(1+sl*sl)
        f=smin(f,(dt, vx/l if l>1e-3 else 0, vy/l if l>1e-3 else 0),M['K'])
    for bx,by,r in M['beads']:
        vx,vy=p[0]-bx,p[1]-by; l=math.hypot(vx,vy)
        f=smin(f,(l-r, vx/l if l>1e-3 else 0, vy/l if l>1e-3 else 0),M['K'])
    return f

def alpha(p,M):
    l,t,r,b=M['hole']
    if l<=p[0]<r and t<=p[1]<b: return 0.0
    f=field(p,M); cov=min(max(0.5-f[0],0),1)
    if cov<=0: return 0.0
    tt=min(max(-f[0]/(BEZ*CH),0),1); s=1-tt; z=math.sqrt(math.sqrt(max(1-s**4,0)))
    z3=z**3; s3=s**3; nz=z3/math.sqrt(max(s3*s3+z3*z3,1e-12)); fr=1-nz; F=0.04+0.96*fr*fr
    gl=math.hypot(f[1],f[2]); n=(f[1]/gl,f[2]/gl) if gl>1e-6 else (0,0)
    bt=min(max((tt-0.04)*2.5,0),1); band=4*bt*(1-bt)
    nl=n[0]*M['light'][0]+n[1]*M['light'][1]
    return M['E']*cov*min(max(A_BODY+A_RIM*F+S_MAIN*max(nl,0)**3*band+S_BACK*max(-nl,0)**3*band,0),1)

def render(name, frames, W, Hh, bg, fg, cursor):
    rows=[]
    for ig,age in frames:
        M=model(ig,age); img=[]
        for y in range(Hh):
            row=[]
            for x in range(W):
                a=alpha((x+0.5,y+0.5),M)
                row.append(int(round(fg*a+bg*(1-a))))
            img.append(row)
        l,t,r,b=M['hole']
        for y in range(int(t),min(int(b),Hh)):
            for x in range(int(l),min(int(r),W)): img[y][x]=cursor
        rows.append(img)
    with open(name+'.ppm','wb') as fo:
        fo.write(b'P6 %d %d 255\n'%(W,Hh*len(rows)))
        for img in rows:
            for row in img: fo.write(bytes(v for c in row for v in (c,c,c)))

def main():
    frm=cell(8,2); to=cell(38,2)
    ig=Ignition(seg_new(corners(frm),to),7)
    print('end',ig.end)
    ages=[0.02,0.06,0.10,0.14,0.18,0.22,0.26,0.30,0.36]
    frames=[(ig,a) for a in ages]
    render('dark', frames, int(44*CW), int(5*CH), 18, 235, 235)
    render('light', frames, int(44*CW), int(5*CH), 239, 30, 30)
    # retarget at 0.12 s from the corners in flight to a cell two rows down
    ig2=Ignition(seg_new(corners(frm),to),7)
    at=0.12; cs=seg_at(ig2.segs[-1][0],at)
    before=model(ig2,at-0.001)
    ig2.retarget(seg_new(cs,cell(30,5)),at)
    after=model(ig2,at+0.001)
    for k in ('B','R','n','light','E','w'):
        print('retarget', k, before[k], '->', after[k])
    print('beads r', [round(b[2],2) for b in before['beads']], '->', [round(b[2],2) for b in after['beads']])
    print('end after retarget', ig2.end)
    render('retarget', [(ig2,a) for a in (0.10,0.119,0.121,0.15,0.2,0.26,0.32,0.4)], int(44*CW), int(8*CH), 18, 235, 235)
main()
