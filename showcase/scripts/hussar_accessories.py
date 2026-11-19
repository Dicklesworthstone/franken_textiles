"""Full-size accessory drafts for the showcase's two Swedish hussar costume interpretations.

All coordinates are finished stitching lines in mm. The export adds each panel's cutting allowance.
The silhouettes follow the exhibition, while the construction and synthetic sizes are original.
"""

from copy import deepcopy
from math import cos, dist, hypot, pi, sin, sqrt, tau


def _piece(key, title, material, path, grain, note, *, qty=1, sa=7, curves=None, marks=()):
    # Each path entry names the edge going from that point to its successor.
    points = [(x, y) for _, x, y in path]
    if sum(a[0]*b[1]-a[1]*b[0] for a, b in zip(points, points[1:]+points[:1])) <= 0:
        raise ValueError("Clockwise or empty stitch line: "+key)
    edges = []
    for i, (tag, x, y) in enumerate(path):
        j = (i+1) % len(path)
        edge = {"id":tag, "from":f"v{i}", "to":f"v{j}"}
        if curves and tag in curves:
            dx, dy = points[j][0]-x, points[j][1]-y
            d2 = dx*dx+dy*dy
            kind, controls = curves[tag]
            relative = [[((a-x)*dx+(b-y)*dy)/d2, ((b-y)*dx-(a-x)*dy)/d2] for a, b in controls]
            edge["curve"] = {"type":kind, "c":relative}
        edges.append(edge)
    return {"id":key, "name":title, "material":material, "cut":{"qty":qty, "material":material},
            "vertices":{f"v{i}":list(p) for i,p in enumerate(points)}, "edges":edges,
            "grain":grain, "note":note, "sa":sa, "sim":False, "internal":list(marks)}


def _mirror_face(piece, key, title, note):
    result = deepcopy(piece)
    result.update(id=key, name=title, note=note)
    for p in result["vertices"].values():
        p[0] *= -1
    for p in result["grain"]:
        p[0] *= -1
    for mark in result["internal"]:
        for p in mark["pts"]:
            p[0] *= -1
    result["edges"].reverse()
    for edge in result["edges"]:
        edge["from"], edge["to"] = edge["to"], edge["from"]
        if "curve" in edge:
            edge["curve"]["c"] = [[1-t,h] for t,h in reversed(edge["curve"]["c"])]
    return result


def _mark(label, points):
    return {"kind":"draw", "label":label, "pts":points, "closed":False}


def _band(key, title, material, lower, upper, slant, note, *, qty=1, sa=7, divisions=4):
    # Annular sector. Cutting a straight-sided trapezoid leaves a kink at a flared rim's seam.
    angle = abs(upper-lower)/slant
    def point(t, rim):
        if angle == 0:
            return [(t-.5)*lower, slant if rim else 0]
        r = (upper if rim else lower)/angle
        return [r*sin((t-.5)*angle), (1 if upper>lower else -1)*(r*cos((t-.5)*angle)-lower/angle)]
    path, curves = [], {}
    def arc(tag, frm, to, rim):
        path.append((tag,*point(frm,rim)))
        if angle:
            curves[tag] = ("poly", [point(frm+(to-frm)*j/24,rim) for j in range(1,24)])
    for i in range(divisions):
        arc(f"base{i}", i/divisions, (i+1)/divisions, False)
    path.append(("end", *point(1,False)))
    for i in reversed(range(divisions)):
        arc(f"top{i}", (i+1)/divisions, i/divisions, True)
    path.append(("start", *point(0,True)))
    marks=[]
    for i in range(1,divisions):
        lower_mark,upper_mark=point(i/divisions,False),point(i/divisions,True)
        for a,b in [(lower_mark,upper_mark),(upper_mark,lower_mark)]:
            marks.append(_mark("",[a,[x+(u-x)*4/slant for x,u in zip(a,b)]]))
    return _piece(key,title,material,path,[[0,slant*.22],[0,slant*.78]],note,
                  qty=qty,sa=sa,curves=curves,marks=marks)


def _oval(key, title, material, perimeter):
    # Real oval crown with four marked quarter seams. Scale the same polygon the export consumes.
    outline = [(cos(i*tau/192-pi/2),1.22*sin(i*tau/192-pi/2)) for i in range(192)]
    scale = perimeter/sum(dist(a,b) for a,b in zip(outline,outline[1:]+outline[:1]))
    pts = [[x*scale,y*scale] for x,y in outline]
    path = [(f"q{i}",*pts[i*48]) for i in range(4)]
    curves = {f"q{i}":("poly",pts[i*48+1:(i+1)*48]) for i in range(4)}
    marks=[_mark("Centre back" if i==0 else "",[pts[i*48],[v*.94 for v in pts[i*48]]]) for i in range(4)]
    return _piece(key,title,material,path,[[0,-46],[0,46]],
                  "Match quarter marks; align long axis front to back. Ease evenly around the turn.",curves=curves,marks=marks)


def _rectangle(key,title,material,w,h,note,*,qty=1,sa=7,marks=()):
    return _piece(key,title,material,[("bottom",0,0),("right",w,0),("top",w,h),("left",0,h)],
                  [[w*.30,h*.5],[w*.67,h*.5]],note,qty=qty,sa=sa,marks=marks)


def accessories(regiment):
    if regiment not in ("blue","yellow"):
        raise ValueError("Expected blue or yellow regiment")
    pieces, seams = [], []
    def join(name, a, b, reverse=True):
        seams.append({"id":name,"a":a,"b":b,"reverse":reverse})
    def ring(key):
        join(key+"_back",key+"/start",key+"/end")
    def rims(a,b,upper):
        for i in range(4):
            join(f"{a}_{b}_q{i}",f"{a}/top{i}",f"{b}/{upper}{i}")
    def pair_seam(name,a,b,reverse=True):
        for side in ("R","L"):
            join(name+"_"+side,a.replace("@","."+side),b.replace("@","."+side),reverse)

    blue = regiment=="blue"
    hat = "mirliton" if blue else "busby"
    wall = hat+"_wall"
    top = hat+("_yellow_crown" if blue else "_upper_wall")
    cap = hat+"_tip"
    sweat = hat+"_sweatband"
    middle, tip = (604,466) if blue else (620,578)
    pieces.extend([
        _band(wall,"Mirliton - lower wall" if blue else "Busby - lower wall",
              "black wool felt" if blue else "black faux fur",590,middle,98 if blue else 90,
              "Join centre-back edges. The 590 mm base allows 10 mm over a 580 mm head."),
        _band(top,"Mirliton - yellow crown wall" if blue else "Busby - upper wall",
              "yellow wool" if blue else "black faux fur",middle,tip,89 if blue else 88,
              "Match the four lower quarters to the lower wall; align both back seams."),
        _oval(cap,"Mirliton - crown tip" if blue else "Busby - crown tip","yellow wool" if blue else "black faux fur",tip),
        _band(sweat,"Hat - inside sweatband","cotton twill",590,590,32,
              "Join short ends. Match lower quarters to the hat base, turn inward and tack."),
    ])
    ring(wall); ring(top); ring(sweat)
    rims(wall,top,"base"); rims(top,cap,"q")
    for i in range(4):
        join(sweat+"_q"+str(i),f"{wall}/base{i}",f"{sweat}/base{i}",False)
    if blue:
        streamer=_piece("mirliton_streamer","Mirliton - yellow streamer","yellow wool",
            [("root",-20,0),("edge_r",20,0),("tip",10,398),("edge_l",-7,420)],
            [[0,65],[0,285]],"Turn long edges narrowly; tack root to the upper wall and drape down the side.",sa=5)
        pieces.append(streamer)
    else:
        bag=_piece("busby_bag","Busby - pendant bag","yellow wool",
            [("mouth",-28,0),("outside",28,0),("end",14,169),("inside",-4,194)],
            [[0,36],[0,129]],"Sew mirrored sides and point. Turn, tuck the mouth inward and tack under the upper band.",
            qty=2,sa=6,curves={"outside":("cubic",[(28,42),(25,128)]),"inside":("cubic",[(-18,144),(-28,44)])})
        pieces.append(bag)
        for edge in ("outside","end","inside"):
            join("bag_"+edge,f"busby_bag.R/{edge}",f"busby_bag.L/{edge}")

    # 202 mm wrist opening, 193 mm wrist-to-middle-tip; one hand's palm and back are oppositely handed.
    # Between two fingers a real fourchette spans the web and tapers to both fingertips.
    points=[("wrist",-50.5,0),("ulnar",50.5,0)]
    digits=[("little",43,108,25,101,151),("ring",25,101,3,112,177),
            ("middle",3,112,-20,114,188),("index",-20,114,-43,103,177)]
    measures={}
    curves={}
    for name,xr,yr,xl,yl,ynail in digits:
        center=(xr+xl)/2
        rtip=(center+(xr-xl)*.37,ynail)
        ltip=(center-(xr-xl)*.37,ynail)
        points.extend([(name+"_r",xr,yr),(name+"_tip",*rtip),(name+"_l",*ltip)])
        curves[name+"_tip"]=("quad",[(center,ynail+9)])
        measures[name+"_r"]=dist((xr,yr),rtip)
        measures[name+"_l"]=dist((xl,yl),ltip)
    points.extend([("radial_top",-43,103),("socket",-43,80),("radial_wrist",-43,37)])
    clips=[_mark("Clip to 0.7 mm from web",[[xl,yl+12],[xl,yl+.7]]) for _,_,_,xl,yl,_ in digits[:-1]]
    clips.extend([_mark("Thumb web",[[-43,80],[-35,80]]),_mark("Thumb base",[[-43,37],[-35,37]])])
    palm=_piece("glove_palm","Gauntlet - palm","supple buff gloving leather",points,
                [[-9,14],[-9,79]],"Stretch across the palm. Use fine 1.5 mm glove seams; clip carefully to the marked webs.",
                qty=2,sa=1.5,curves=curves,marks=clips)
    back=_mirror_face(palm,"glove_back","Gauntlet - back","Pair one back with the opposite-facing palm from the same hand. Use the named finger edges.")
    pieces.extend([palm,back])
    for e in ["ulnar","little_r","index_l","radial_top","radial_wrist"]+[n+"_tip" for n,*_ in digits]:
        pair_seam("glove_"+e,"glove_palm@/"+e,"glove_back@/"+e)
    for i,(name,*_) in enumerate(digits[:-1]):
        nxt=digits[i+1][0]
        a,b=name+"_l",nxt+"_r"
        half=[5.5,6.5,7][i]
        x1=sqrt(measures[a]**2-half**2)
        x2=x1+sqrt(measures[b]**2-half**2)
        key=f"web_{name}_{nxt}"
        pieces.append(_piece(key,f"Finger web - {name} / {nxt}","supple buff gloving leather",
                      [("palm_a",0,0),("palm_b",x1,-half),("back_b",x2,0),("back_a",x1,half)],
                      [[x1-11,0],[x1+11,0]],"Widest point at the finger web. Taper each end into its named fingertip.",qty=2,sa=1.5))
        for piece,labels in [(palm,[(a,"V"),(b,"double")]),(back,[(a,"V"),(b,"double")]),
                             (pieces[-1],[("palm_a","V"),("back_a","V"),("palm_b","double"),("back_b","double")])]:
            piece.setdefault("notches",[]).extend({"id":"match_"+edge,"edge":edge,"s":.5,
                "type":kind,"depth":.65,"width":1.2} for edge,kind in labels)
        pair_seam(key+"_pa",key+"@/palm_a","glove_palm@/"+a,False)
        pair_seam(key+"_pb",key+"@/palm_b","glove_palm@/"+b,False)
        pair_seam(key+"_ba",key+"@/back_a","glove_back@/"+a,False)
        pair_seam(key+"_bb",key+"@/back_b","glove_back@/"+b,False)

    # The asymmetric-depth gore rotates the opposed thumb away from the palm plane.
    gusset=_piece("thumb_gusset","Gauntlet - thumb socket gusset","supple buff gloving leather",
        [("palm_open",0,0),("back_open",43,0),("join_r",86,0),("back_thumb",74,15),
         ("palm_thumb",43,31),("join_l",12,15)],[[35,9],[53,9]],
        "Close the short ends. Set the 86 mm lower ring into the two thumb openings. Place the tall point toward the thumb web.",qty=2,sa=3)
    pieces.append(gusset)
    half=hypot(31,16)/2
    thumb=_piece("thumb_palm","Gauntlet - thumb palm","supple buff gloving leather",
        [("root",-half,0),("outer",half,0),("tip",10,54),("inner",-10,54)],[[0,8],[0,41]],
        "Set the root to the palm half of the thumb gusset. Close both thumb faces around the tip.",qty=2,sa=3,
        curves={"tip":("quad",[(0,68)])})
    pieces.extend([thumb,_mirror_face(thumb,"thumb_back","Gauntlet - thumb back","Set this root to the back half of the thumb gusset.")])
    pair_seam("thumb_gore_close","thumb_gusset@/join_l","thumb_gusset@/join_r")
    pair_seam("thumb_hole_palm","glove_palm@/socket","thumb_gusset@/palm_open")
    pair_seam("thumb_hole_back","glove_back@/socket","thumb_gusset@/back_open")
    pair_seam("thumb_root_palm","thumb_palm@/root","thumb_gusset@/palm_thumb")
    pair_seam("thumb_root_back","thumb_back@/root","thumb_gusset@/back_thumb")
    for e in ("outer","tip","inner"):
        pair_seam("thumb_"+e,"thumb_palm@/"+e,"thumb_back@/"+e)
    cuff=_band("glove_cuff","Gauntlet - flared cuff","buff gloving leather",292,202,82,
               "Close cuff seam; match the 101 mm wrist halves to the palm and back. Bind the 292 mm flared opening.",qty=2,sa=4,divisions=2)
    pieces.append(cuff)
    pair_seam("cuff_close","glove_cuff@/start","glove_cuff@/end")
    pair_seam("cuff_palm","glove_palm@/wrist","glove_cuff@/top0")
    pair_seam("cuff_back","glove_back@/wrist","glove_cuff@/top1")

    stock_note="Sew lining at both long edges; turn and finish ends. Fit tabs at the marked positions for a 39-43 cm neck."
    marks=[_mark("Closure position",[[x,8],[x,34]]) for x in (12,402,422,442)]
    for key,label,fabric in [("stock_face","Neck stock - face","black wool"),("stock_lining","Neck stock - lining","cotton lawn")]:
        pieces.append(_rectangle(key,label,fabric,455,45,stock_note,marks=marks))
    for e in ("bottom","top"):
        join("stock_"+e,"stock_face/"+e,"stock_lining/"+e,False)
    cord_lines=[_mark("Cord row",[[12,5+4*i],[878,5+4*i]]) for i in range(12)]
    barrel_marks=[_mark("Barrel wrap",[[27+49*i,4],[27+49*i,50]]) for i in range(18)]
    pieces.append(_rectangle("sash_face","Barrel sash - cord foundation","matching wool",900,54,
        "Couch twelve cord rows; bind them in eighteen marked barrel groups. Work this face before adding the backing.",
        marks=cord_lines+barrel_marks))
    pieces.append(_rectangle("sash_back","Barrel sash - backing","firm linen",900,54,
        "Join both long edges to the corded face; turn, finish the ends and add the ties over the dolman."))
    for e in ("bottom","top"):
        join("sash_"+e,"sash_face/"+e,"sash_back/"+e,False)
    tie=_rectangle("sash_tie","Barrel sash - ties","matching wool",265,26,
        "Fold lengthwise, sew the long edges and turn. Secure one tie to each end of the sash. Add a cord tassel.",
        qty=2,sa=5)
    pieces.append(tie)
    pair_seam("sash_tie_close","sash_tie@/bottom","sash_tie@/top")
    for key,width,label in [("crossbelt_wide",51,"broad"),("crossbelt_narrow",22,"narrow")]:
        pieces.append(_rectangle(key,"Costume crossbelt - "+label,"buff leather",690,width,
            "Butt-stitch the shoulder ends. Fit the hip overlap and secure with costume snaps. Finish the long edges.",
            qty=2,sa=0,marks=[_mark("Overlap range",[[x,3],[x,width-3]]) for x in (36,118)]))
        join(key+"_shoulder",key+".R/right",key+".L/right")
    return pieces,seams
