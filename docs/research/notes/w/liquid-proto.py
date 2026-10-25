# Prototype of the proposed liquid trail (design (a)): the Rust-side model and
# the fragment, in plain Python, to check the look and the reach bound.
# Not repo code. Renders frames of a 30-column hop to PPM.
import math, sys

CW, CH = 17.0, 34.0
TAU_FAST, TAU_SLOW = 0.1, 0.4

def pcg(v):
    v &= 0xffffffff
    s = (v * 747796405 + 2891336453) & 0xffffffff
    w = (((s >> ((s >> 28) + 4)) ^ s) * 277803737) & 0xffffffff
    return (w >> 22) ^ w
def unit(h): return (h >> 9) / float(1 << 23)

def cell(c, r): return (c*CW, r*CH, (c+1)*CW, (r+1)*CH)
def corners(rect):  # kitty order tr, br, bl, tl
    l,t,r,b = rect; return [(r,t),(r,b),(l,b),(l,t)]
# The rates as note W, section 8.4, derives them: a corner leads by the
# cosine of its travel and its outward direction; one that does not travel
# takes no part; the leads' spread runs from TAU_FAST to TAU_SLOW.
def lead(at, to, c):
    dx,dy = to[0]-at[0], to[1]-at[1]; ox,oy = to[0]-c[0], to[1]-c[1]
    travel,out = math.hypot(dx,dy), math.hypot(ox,oy)
    return (dx*ox+dy*oy)/(travel*out) if travel>=1e-3 and out>0 else None
def seg_new(frm, to):
    tgt = corners(to); c = ((to[0]+to[2])/2, (to[1]+to[3])/2)
    leads=[lead(frm[i],tgt[i],c) for i in range(4)]
    live=[l for l in leads if l is not None]
    lo,hi=(min(live),max(live)) if live else (0.0,0.0)
    def rate(l):
        if l is None or hi-lo<=1e-7: return TAU_SLOW
        s=(l-lo)/(hi-lo); return TAU_FAST*s+TAU_SLOW*(1-s)
    return (frm,to,[rate(l) for l in leads])
def seg_at(seg,t):
    frm,to,tau=seg; tgt=corners(to); out=[]
    for i in range(4):
        k=2.0**(-10*t/tau[i]); out.append((tgt[i][0]+(frm[i][0]-tgt[i][0])*k, tgt[i][1]+(frm[i][1]-tgt[i][1])*k))
    return out

def smoothstep(a,b,x):
    t=min(max((x-a)/(b-a),0.0),1.0); return t*t*(3-2*t)

# constants (cells are CH)
G=0.10; AMAX=0.08; K=0.15; BEZ=0.25; NECK=1.45
A_BODY,A_RIM,S_MAIN,S_BACK=0.12,0.55,0.85,0.25
ZETA, FREQ = 0.28, 6.5
FADE_IN, FADE_OUT = 0.035, 0.10

def spine(cs, hole):
    H=((hole[0]+hole[2])/2,(hole[1]+hole[3])/2)
    m=(sum(c[0] for c in cs)/4, sum(c[1] for c in cs)/4)
    vx,vy=H[0]-m[0],H[1]-m[1]; L=math.hypot(vx,vy)
    if L<0.5: return H,H,(1.0,0.0),False
    d=(vx/L,vy/L)
    pr=sorted((c[0]-H[0])*d[0]+(c[1]-H[1])*d[1] for c in cs)
    sb=0.5*(pr[0]+pr[1])
    return (H[0]+d[0]*sb,H[1]+d[1]*sb),H,d,True

def model(seg, since, age, end, seed):
    cs=seg_at(seg,since); hole=seg[1]
    B,H,d,on=spine(cs,hole)
    B0,_,d0,_=spine(seg_at(seg,0.0),hole)
    L0=math.hypot(H[0]-B0[0],H[1]-B0[1])
    R=0.45*math.hypot(d0[1]*CW,d0[0]*CH)
    neckmax=NECK*smoothstep(1.5,4.0,L0/CH)
    n=neckmax*smoothstep(0.03,0.14,since)
    # contact
    tgt=corners(hole); tc=0.0
    for i in range(4):
        gap=max(abs(tgt[i][0]-seg[0][i][0]),abs(tgt[i][1]-seg[0][i][1]))
        if gap>R+G*CH: tc=max(tc, seg[2][i]/10*math.log2(gap/(R+G*CH)))
    A=AMAX*CH*smoothstep(3.0,12.0,L0/CH)
    w=0.0; s=since-tc
    om=2*math.pi*FREQ; od=om*math.sqrt(1-ZETA*ZETA)
    if s>0: w=A*math.exp(-ZETA*om*s)*math.sin(od*s)
    hx,hy=(hole[2]-hole[0])/2,(hole[3]-hole[1])/2
    bx=hx+G*CH+w*abs(d0[0])-0.6*w*abs(d0[1]); by=hy+G*CH+w*abs(d0[1])-0.6*w*abs(d0[0])
    rho=min(G*CH+A+1.0,bx,by)
    # beads
    beads=[]
    if neckmax>1.0:
        y=1.0/neckmax; xp=0.5-math.sin(math.asin(1-2*y)/3); tp=0.03+0.11*xp
        h=pcg(seed^0x5bd1e995); cnt=2+(h&1)
        e=(H[0]-B[0],H[1]-B[1]); perp=(-d0[1],d0[0])
        for j in range(cnt):
            h0=pcg(seed ^ ((j+1)*0x9E3779B9 & 0xffffffff)); h1=pcg(h0); h2=pcg(h1); h3=pcg(h2)
            sg=since-tp
            if sg<0: continue
            u0=0.5+(unit(h0)-0.5)*0.16; tj=0.045+0.04*unit(h2)
            u=u0+(1-u0)*(1-math.exp(-sg/tj))
            r=R*(0.30-0.07*j)*(0.85+0.3*unit(h1))*(1-smoothstep(0.85,0.99,u))
            o=(unit(h3)-0.5)*0.25*R*math.exp(-sg/0.06)
            if r>0: beads.append((B[0]+e[0]*u+perp[0]*o, B[1]+e[1]*u+perp[1]*o, r))
    Lx,Ly=-0.5,-0.866; sw=1.2*math.exp(-since/0.08); Lx+=sw*d0[0]; Ly+=sw*d0[1]; ll=math.hypot(Lx,Ly); light=(Lx/ll,Ly/ll)
    E=smoothstep(0,FADE_IN,age)*(1-smoothstep(end-FADE_OUT,end,age))
    Tw = math.log(A/0.5)/(ZETA*om) if A>0.5 else 0.0
    return dict(B=B,H=H,on=on,R=R,n=n,box=(bx,by),rho=rho,K=K*CH,beads=beads,light=light,E=E,hole=hole,tc=tc,Tw=Tw,
                reach=0.45*max(CW,CH)+K*CH+1.0, L0=L0)

def smin(a,b,k):
    h=1.0-min(abs(a[0]-b[0])/(4*k),1.0); s=h*h*k; wb=0.5*h
    if a[0]<b[0]: return (a[0]-s, a[1]+(b[1]-a[1])*wb, a[2]+(b[2]-a[2])*wb)
    return (b[0]-s, b[1]+(a[1]-b[1])*wb, b[2]+(a[2]-b[2])*wb)

def field(p,M):
    H=M['H']; hx,hy=M['box']; rho=M['rho']
    px,py=p[0]-H[0],p[1]-H[1]
    sx=1.0 if px>=0 else -1.0; sy=1.0 if py>=0 else -1.0
    wx,wy=abs(px)-(hx-rho),abs(py)-(hy-rho); g=max(wx,wy)
    if g>0:
        ax,ay=max(wx,0),max(wy,0); lw=math.hypot(ax,ay); d=lw-rho; gr=(sx*ax/lw,sy*ay/lw)
    else:
        d=g-rho; gr=(sx,0.0) if wx>wy else (0.0,sy)
    f=(d,gr[0],gr[1])
    if M['on']:
        B=M['B']; e=(H[0]-B[0],H[1]-B[1]); L2=e[0]**2+e[1]**2
        if L2>0.25:
            u=min(max(((p[0]-B[0])*e[0]+(p[1]-B[1])*e[1])/L2,0),1)
            vx,vy=p[0]-(B[0]+e[0]*u),p[1]-(B[1]+e[1]*u); l=math.hypot(vx,vy)
            q=1-4*M['n']*u*(1-u); lip=4*M['n']*M['R']/math.sqrt(L2); sl=lip*(2*u-1)
            dt=(l-M['R']*q)/math.sqrt(1+sl*sl)
            f=smin(f,(dt, vx/l if l>1e-3 else 0, vy/l if l>1e-3 else 0),M['K'])
    for bx,by,r in M['beads']:
        vx,vy=p[0]-bx,p[1]-by; l=math.hypot(vx,vy)
        f=smin(f,(l-r, vx/l if l>1e-3 else 0, vy/l if l>1e-3 else 0),M['K'])
    return f

def shade(p,M):
    l,t,r,b=M['hole']
    if l<=p[0]<r and t<=p[1]<b: return 0.0
    f=field(p,M); cov=min(max(0.5-f[0],0),1)
    if cov<=0: return 0.0
    tt=min(max(-f[0]/(BEZ*CH),0),1); s=1-tt; s4=s**4; hh=math.sqrt(math.sqrt(max(1-s4,0)))
    h3=hh**3; s3=s**3; nz=h3/math.sqrt(max(s3*s3+h3*h3,1e-12)); fr=1-nz; F=0.04+0.96*fr*fr
    gl=math.hypot(f[1],f[2]); n=(f[1]/gl,f[2]/gl) if gl>1e-6 else (0,0)
    bt=min(max((tt-0.04)*2.5,0),1); band=4*bt*(1-bt)
    nl=n[0]*M['light'][0]+n[1]*M['light'][1]
    sp=max(nl,0)**3*band; bk=max(-nl,0)**3*band
    return M['E']*cov*min(max(A_BODY+A_RIM*F+S_MAIN*sp+S_BACK*bk,0),1)

def main():
    frm=cell(8,2); to=cell(38,2)
    seg=seg_new(corners(frm),to)
    M0=model(seg,0,0,1,7)
    end=M0['tc']+M0['Tw']+0.5*FADE_OUT
    print('L0',M0['L0'],'R',M0['R'],'tc',M0['tc'],'Tw',M0['Tw'],'end',end,'reach',M0['reach'])
    W,Hh=int(44*CW),int(5*CH)
    frames=[0.02,0.06,0.10,0.14,0.18,0.24,0.30,0.36]
    rows=[]
    maxreach=0
    for tt in frames:
        M=model(seg,tt,tt,end,7)
        img=[]
        for y in range(Hh):
            row=[]
            for x in range(W):
                p=(x+0.5,y+0.5)
                a=shade(p,M)
                if a>0:
                    # distance to cores
                    B,H=M['B'],M['H']; ex,ey=H[0]-B[0],H[1]-B[1]; L2=ex*ex+ey*ey
                    u=0 if L2<1e-6 else min(max(((p[0]-B[0])*ex+(p[1]-B[1])*ey)/L2,0),1)
                    ds=math.hypot(p[0]-(B[0]+ex*u),p[1]-(B[1]+ey*u))
                    l,t,r,b=M['hole']; dx=max(l-p[0],0,p[0]-r); dy=max(t-p[1],0,p[1]-b); dh=math.hypot(dx,dy)
                    maxreach=max(maxreach,min(ds,dh))
                c=int(round(18+(235-18)*a)); row.append(c)
            img.append(row)
        # draw cursor block
        l,t,r,b=to
        for y in range(int(t),int(b)):
            for x in range(int(l),int(r)): img[y][x]=235
        rows.append((tt,M,img))
        print('t=%.2f B=(%.0f,%.0f) n=%.2f beads=%d w-box=(%.1f,%.1f) E=%.2f'%(tt,M['B'][0],M['B'][1],M['n'],len(M['beads']),M['box'][0],M['box'][1],M['E']))
    print('max core distance of a drawn pixel',maxreach,'vs reach',M0['reach'])
    with open('liquid.ppm','wb') as fo:
        fo.write(b'P6 %d %d 255\n'%(W,Hh*len(rows)))
        for _,_,img in rows:
            for row in img:
                fo.write(bytes(v for c in row for v in (c,c,c)))
if __name__ == "__main__": main()
