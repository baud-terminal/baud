import sys; sys.argv=['x']
exec(open('proto3.py').read().split("if __name__ == '__main__':")[0])
c, cw = 34.0, 17.0
moves=[((3,6),(28,5)),((29,7),(4,2)),((5,6),(25,0)),((16,8),(17,1)),((10,4),(14,4)),((0,8),(99,0)),((0,0),(60,7))]
res=[]
for frm,to in moves:
    fr,tr=cellrect(*frm,cw,c),cellrect(*to,cw,c); seg=Segment(rect_corners(fr),tr); worst=0; wa=0
    for i in range(0,73,2):
        a=i*0.01; f=uniform(seg.at(a),tr,centre(fr),centre(tr),c,a,7)
        x0,y0,x1,y1=region(f)
        for y in range(int(y0),int(y1)+1,2):
            for x in range(int(x0),int(x1)+1,2):
                p=(x+.5,y+.5)
                if ufo_band(p,f)>0:
                    d=quad_dist(p,f['c'])/c
                    if d>worst: worst,wa=d,a
    res.append((frm,to,round(worst,2),wa)); print(frm,to,'farthest lit pixel from kitty quad:',round(worst,2),'units at age',wa)
