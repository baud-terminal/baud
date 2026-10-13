import math
D,W=2.5,2*math.pi*2.0
def closed(v,a):
    E=math.exp(-D*a);C=math.cos(W*a);S=math.sin(W*a);den=D*D+W*W
    mr=(D*(1-E*C)+W*E*S)/den; mi=(W*(1-E*C)-D*E*S)/den
    return (mr*v[0]-mi*v[1], mi*v[0]+mr*v[1])
def numeric(v,a,n=200000):
    x=y=0;dt=a/n;vx,vy=v
    for i in range(n):
        t=(i+0.5)*dt;E=math.exp(-D*t);c=math.cos(W*t);s=math.sin(W*t)
        x+=E*(c*vx-s*vy)*dt;y+=E*(s*vx+c*vy)*dt
    return x,y
for a in (0.1,0.3,0.55):print(a,closed((3,1),a),numeric((3,1),a))
print("reach d:", math.log(0.20/(0.5/255))/5.0)
# light-theme flash width multiplier: exp(-e^2)>=0.094
print("dark core half", math.sqrt(-math.log(0.35)), "light half", math.sqrt(-math.log(0.094)))
print("srgb .28 lin", ((0.28+0.055)/1.055)**2.4, "srgb .35 lin", ((0.35+0.055)/1.055)**2.4)
print("gyr radius", 1.8/math.hypot(D,W), 4.2/math.hypot(D,W))
