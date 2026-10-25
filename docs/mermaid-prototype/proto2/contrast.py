def lum(rgb):
    r,g,b = [((rgb>>s)&255)/255 for s in (16,8,0)]
    f = lambda c: c/12.92 if c <= 0.03928 else ((c+0.055)/1.055)**2.4
    return 0.2126*f(r)+0.7152*f(g)+0.0722*f(b)
def cr(a,b):
    la,lb = lum(a),lum(b)
    hi,lo = max(la,lb),min(la,lb)
    return (hi+0.05)/(lo+0.05)
LEVELS=[0,95,135,175,215,255]
def n256(rgb):
    r,g,b=[(rgb>>s)&255 for s in (16,8,0)]
    lv=lambda v: min(range(6), key=lambda i: abs(LEVELS[i]-v))
    ri,gi,bi=lv(r),lv(g),lv(b)
    cube=(LEVELS[ri],LEVELS[gi],LEVELS[bi]); ci=16+36*ri+6*gi+bi
    avg=(r+g+b)//3; gs=min(max(avg-8,0)//10,23); gv=8+10*gs
    d=lambda a: sum((x-y)**2 for x,y in zip(a,(r,g,b)))
    if d((gv,gv,gv)) < d(cube): return 232+gs, (gv<<16)|(gv<<8)|gv
    return ci, (cube[0]<<16)|(cube[1]<<8)|cube[2]
bgs={'white':0xFFFFFF,'onedark 282C34':0x282C34,'black 000000':0x000000,'1E1E1E':0x1E1E1E}
cols={'light border C4CBD3':(0xC4CBD3,'white'),'light muted 6E7781':(0x6E7781,'white'),'light 57606A':(0x57606A,'white'),'light 8C959F':(0x8C959F,'white'),
 'dark border 4B5263 on 282C34':(0x4B5263,'onedark 282C34'),'dark border 4B5263 on 1E1E1E':(0x4B5263,'1E1E1E'),'dark border 4B5263 on black':(0x4B5263,'black 000000'),
 'dark muted 7F848E on 282C34':(0x7F848E,'onedark 282C34'),'dark 9DA5B4 on 282C34':(0x9DA5B4,'onedark 282C34'),'dark ABB2BF on 282C34':(0xABB2BF,'onedark 282C34'),'dark 9DA5B4 on black':(0x9DA5B4,'black 000000'),'dark 7F848E on black':(0x7F848E,'black 000000')}
for k,(c,bg) in cols.items():
    idx,approx=n256(c)
    print(f"{k:34s} truecolor {cr(c,bgs[bg]):4.2f}:1   256-color idx {idx} #{approx:06X} {cr(approx,bgs[bg]):4.2f}:1")
