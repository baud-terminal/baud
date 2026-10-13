# Black-body colour ramp, derived: Planck's law x CIE 1931 CMFs (Wyman-Sloan-Shirley 2013
# multi-lobe fit, papers/cmf.txt lines 238-259) -> XYZ -> linear sRGB (CSS Color 4
# conversions.js:62-72) -> sRGB transfer (conversions.js:31-45). Pure math, no copied code.
import math
h,c,k = 6.62607015e-34, 2.99792458e8, 1.380649e-23
def planck(lam_nm, T):
    l = lam_nm*1e-9
    return (2*h*c*c/l**5) / (math.exp(h*c/(l*k*T)) - 1.0)
def g(x, mu, s1, s2):
    t = (x-mu)*(s1 if x<mu else s2); return math.exp(-0.5*t*t)
def xbar(w): return 0.362*g(w,442.0,0.0624,0.0374)+1.056*g(w,599.8,0.0264,0.0323)-0.065*g(w,501.1,0.0490,0.0382)
def ybar(w): return 0.821*g(w,568.8,0.0213,0.0247)+0.286*g(w,530.9,0.0613,0.0322)
def zbar(w): return 1.217*g(w,437.0,0.0845,0.0278)+0.681*g(w,459.0,0.0385,0.0725)
M = [[12831/3959,-329/214,-1974/3959],[-851781/878810,1648619/878810,36519/878810],[705/12673,-2585/12673,705/667]]
def XYZ(T):
    X=Y=Z=0.0
    for w in range(380,781,1):
        B=planck(w,T); X+=B*xbar(w); Y+=B*ybar(w); Z+=B*zbar(w)
    return X,Y,Z
def gam(v):
    return 1.055*v**(1/2.4)-0.055 if v>0.0031308 else 12.92*v
Tref = 3000.0
Yref = XYZ(Tref)[1]
rows=[]
N=16
for i in range(N):
    T = 800 + (3000-800)*i/(N-1)
    X,Y,Z = XYZ(T)
    rgb = [sum(M[r][j]*v for j,v in enumerate((X,Y,Z))) for r in range(3)]
    rgb = [max(x,0.0) for x in rgb]          # clip out-of-gamut negatives (DERIVED choice)
    m = max(rgb); chrom = [x/m for x in rgb] # chromaticity, max channel 1
    rel = Y/Yref                             # radiance relative to 3000 K
    s = [gam(x) for x in chrom]
    redness = chrom[0]/sum(chrom)
    rows.append((T, chrom, rel, s, redness))
    print(f"T={T:6.0f}K  linRGB(max1)=({chrom[0]:.3f},{chrom[1]:.3f},{chrom[2]:.4f})  Y/Y3000={rel:.2e}  sRGB8=({round(s[0]*255):3d},{round(s[1]*255):3d},{round(s[2]*255):3d})  R/(R+G+B)={redness:.2f}")
print("// MSL constant table, sRGB-encoded chromaticity, 800..3000 K in", N, "steps")
print("constant half3 BB[%d] = {" % N + ", ".join("half3(%.3f,%.3f,%.3f)" % tuple(r[3]) for r in rows) + "};")
