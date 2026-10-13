exec(open("blackbody.py").read().split("Tref = 3000.0")[0])
N=16
rows=[]
for i in range(N):
    hh=i/(N-1); T=800+5700*hh*hh
    X,Y,Z=XYZ(T)
    rgb=[max(sum(M[r][j]*v for j,v in enumerate((X,Y,Z))),0.0) for r in range(3)]
    m=max(rgb); chrom=[x/m for x in rgb]; s=[gam(x) for x in chrom]
    rows.append(s); red=chrom[0]/sum(chrom)
    print(f"heat={hh:.3f} T={T:6.0f}K sRGB8=({round(s[0]*255):3d},{round(s[1]*255):3d},{round(s[2]*255):3d}) R/(R+G+B)lin={red:.2f}")
print("constant float3 BB[16] = {" + ", ".join("float3(%.3f,%.3f,%.3f)"%tuple(r) for r in rows) + "};")
