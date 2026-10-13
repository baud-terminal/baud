import math
AMAX=21824.0          # 25% of 341x256 CSS px (WCAG 2.3.1 understanding)
T={"dark":0.35,"light":0.094}   # encoded-alpha thresholds (see check.py)
WREL=0.05             # widest core, cells (k=0)
MINPX=1.1
def width_css(g,t,unit,scale):
    if g<=t: return 0.0
    wpx=max(WREL*unit,MINPX)
    return 2*math.sqrt(math.log(g/t))*wpx/scale
def area(g,L_px,t,unit,scale): return 3*width_css(g,t,unit,scale)*L_px/scale
def gain_uncapped(L_px,unit): return 0.95*min(1,(80/(L_px/unit))**2)
def gain_capped(L_px,unit,scale,margin=0.8):
    wcss=max(WREL*unit,MINPX)/scale; q=margin*AMAX/(6*wcss*(L_px/scale))
    return min(0.95, T["light"]*math.exp(min(q*q,50)))
print("cellH_css scale | worst area/AMAX: cells-cap(dark,light)  px-cap(dark,light)")
for hcss in (14,17,20,24,28,34,40):
  for scale in (1,2):
    unit=hcss*scale
    Ls=[unit*x/4 for x in range(8,4*260)]
    w=lambda f,t: max(area(f(L),L,t,unit,scale) for L in Ls)/AMAX
    a1=w(lambda L:gain_uncapped(L,unit),T["dark"]); a2=w(lambda L:gain_uncapped(L,unit),T["light"])
    b1=w(lambda L:gain_capped(L,unit,scale),T["dark"]); b2=w(lambda L:gain_capped(L,unit,scale),T["light"])
    print(f"{hcss:4} {scale} | {a1:5.2f} {a2:5.2f}   {b1:5.2f} {b2:5.2f}")
# what length does the px cap start dimming, 17css@2x
unit=34;scale=2
for Lc in (30,60,100,150,200):
    L=Lc*unit; print(Lc,"cells gain",round(gain_capped(L,unit,scale),3))
