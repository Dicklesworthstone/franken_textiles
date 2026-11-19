"""Draft reconstructed hussar clothing in millimetres and print it using FrankenTextiles."""
import argparse
import json
import math
import shutil
import subprocess
import tempfile
from pathlib import Path

from hussar_accessories import accessories


def length(a, b, controls=()):
    points = [a, *controls, b]
    if len(points) == 2:
        return math.dist(a, b)
    def sample(t):
        row = points
        while len(row) > 1:
            row = [tuple(x+(y-x)*t for x, y in zip(p, q)) for p, q in zip(row, row[1:])]
        return row[0]
    samples = [sample(i/199) for i in range(200)]
    return sum(math.dist(p, q) for p, q in zip(samples, samples[1:]))


def split_walk(a, b, controls, distance):
    """Cut the original Bézier at walked distance; de Casteljau preserves both child curves."""
    if not 0 < distance < length(a,b,controls):
        raise ValueError("Seam division lies outside the curve")
    def split(t):
        row=[a,*controls,b]
        left,right=[row[0]],[row[-1]]
        while len(row)>1:
            row=[tuple(x+(y-x)*t for x,y in zip(p,q)) for p,q in zip(row,row[1:])]
            left.append(row[0]);right.append(row[-1])
        return left[-1],left[1:-1],right[-2:0:-1]
    lo,hi=0,1
    for _ in range(34):
        t=(lo+hi)*.5
        p,c,_=split(t)
        if length(a,p,c)<distance: lo=t
        else: hi=t
    return split((lo+hi)*.5)


def panel(pid, title, nodes, qty, material, note, curves=None, sa=10, mirror=False):
    """Each named node starts the edge of that name. Converts absolute Beziers to FTX chords."""
    curves = curves or {}
    def point(xy):
        return [-xy[0] if mirror else xy[0], xy[1]]
    coords = [point(p) for _, p in nodes]
    ccw = sum(a[0]*b[1]-b[0]*a[1] for a,b in zip(coords, coords[1:]+coords[:1])) > 0
    edges = []
    for i, (name, _) in enumerate(nodes):
        j = (i+1) % len(nodes)
        a,b = coords[i],coords[j]
        controls = [point(p) for p in curves.get(name, [])]
        start,end = i,j
        if not ccw:
            a,b,start,end = b,a,j,i
            controls.reverse()
        dx,dy = b[0]-a[0], b[1]-a[1]
        d2 = dx*dx+dy*dy
        e = {"id":name,"from":f"v{start}","to":f"v{end}"}
        if controls:
            rel = [[((x-a[0])*dx+(y-a[1])*dy)/d2, (-(x-a[0])*dy+(y-a[1])*dx)/d2] for x,y in controls]
            e["curve"] = {"type":"quad" if len(rel)==1 else "cubic","c":rel}
        edges.append(e)
    xs,ys = zip(*coords)
    x=(min(xs)+max(xs))/2
    y=(min(ys)+max(ys))/2
    g=min(50,(max(ys)-min(ys))*.22)
    return {"id":pid, "name":title,"vertices":{f"v{i}":c for i,c in enumerate(coords)},
            "edges":edges if ccw else edges[::-1], "cut":{"qty":qty,"material":material}, "sa":sa,
            "grain":[[x,y-g],[x,y+g]], "note":note}


def stitch(seams, tag, a, b, **options):
    seams.append({"id":tag,"a":a,"b":b,"reverse":True,**options})


def rectangle(pid, title, width, height, qty, material, note):
    return panel(pid,title,[("bottom",(0,0)),("right",(width,0)),("top",(width,height)),("left",(0,height))],qty,material,note)


def coats(regiment, outer):
    prefix = "pelisse" if outer else "dolman"
    jacket = prefix.title()
    cloth = ("blue wool" if regiment=="blue" else ("chamois wool" if outer else "black wool"))
    lining = ("black lamb" if regiment=="blue" else "white lamb") if outer else "linen"
    material=cloth+" / "+lining
    pieces,seams=[],[]
    def add(pid,name,nodes,qty=2,note="Cut a mirrored pair in outer cloth and a pair in lining.",**kw):
        pieces.append(panel(prefix+"_"+pid,jacket+" "+name,nodes,qty,kw.pop("material",material),note,**kw))
    def join(tag,a,b,**kw):
        stitch(seams,prefix+"_"+tag,prefix+"_"+a,prefix+"_"+b,**kw)
    # Horizontal dimensions are distributed from CF to side, side-back and CB.
    grow = 13 if outer else 0
    fw,fb=249+grow,286+grow
    sw,sb=74+grow*.4,91+grow*.4
    bw=148+grow*.6
    under_y,neck_y=240+(12 if outer else 0),457
    fneck=(76+(4 if outer else 0),462)
    shoulder=(fneck[0]+139+grow,425)
    front_arm=[(fb-52,under_y+7),(shoulder[0]-20,376)]
    bn=(83+(5 if outer else 0),neck_y+4)
    back_neck=[(bn[0]*.7,451),(15,450)]
    back_neck_walk=length(bn,(0,450),back_neck)
    collar=432 if outer else 412
    lo,hi=346,411
    for _ in range(32):
        cf_y=(lo+hi)/2
        neck_curve=[(fneck[0]-7,cf_y+.35*(462-cf_y)),(fneck[0]*.42,cf_y)]
        if length(fneck,(0,cf_y),neck_curve)>collar/2-back_neck_walk: lo=cf_y
        else: hi=cf_y
    fw_neck=length(fneck,(0,cf_y),neck_curve)
    # Side-back division passes through the rear armscye, with matching shaped longitudinal seams.
    side_inner=(0,under_y+38)
    side_outer=(sb,under_y)
    side_curve=[(sb-28,under_y+4),(25,under_y+19)]
    # Transfer the front side-seam slope to the back. Keep the upper side-back width.
    side_dx=fb-fw
    side_shift=sw+side_dx-sb
    side_nodes=[("hem",(side_shift,0)),("side",(sw,0)),("armscye",(sw+side_dx,under_y)),
                ("back_join",(side_shift,under_y+38))]
    # The translated side-back seam has identical length on the central back panel.
    back_edge=(bw,under_y+38)
    back_shoulder=(bn[0]+shoulder[0]-fneck[0],bn[1]-37)
    back_arm=[(bw+36,under_y+64),(back_shoulder[0]-6,365)]
    add("back","centre back",[("hem",(0,0)),("side_back",(bw,0)),("armscye",back_edge),
         ("shoulder",back_shoulder),("neck",bn),("center",(0,450))],
         curves={"side_back":[(bw-4,85),(bw-9,172)],"armscye":back_arm,"neck":back_neck})
    front_walk=length((fb,under_y),shoulder,front_arm)
    back_walk=length(side_outer,side_inner,side_curve)+length(back_edge,back_shoulder,back_arm)
    # Divide the crescent underarm and upper sleeve at front/elbow seams. Walk each
    # cap half to its matching armscye, allowing two percent ease in broadcloth.
    a,b=116+grow*.5,132+grow*.5
    w=57+grow*.35
    uf,ub=(-w,24),(w,24)
    trough=(0,0)
    cuf,cub=[(-w*.5,8)],[(w*.5,8)]
    fu,bu=length(uf,trough,cuf),length(trough,ub,cub)
    def upper(span,height,left):
        sign=-1 if left else 1
        return [(sign*span*.70,32),(sign*span*.48,height+6)]
    def walk(span,h,left):
        return length(((-1 if left else 1)*span,24),(0,h),upper(span,h,left))
    def half_width(target,height,left):
        lo,hi=1,350
        for _ in range(32):
            span=(lo+hi)/2
            if walk(span,height,left)<target: lo=span
            else: hi=span
        return (lo+hi)/2
    lo,hi=38,280
    for _ in range(32):
        h=(lo+hi)/2
        fa=half_width(front_walk*1.02-fu,h,True)
        ba=half_width(back_walk*1.02-bu,h,False)
        if fa+ba>a+b: lo=h
        else: hi=h
    h=(lo+hi)/2
    peak=(0,h)
    # Sleeve length measured from shoulder. Both longitudinal seams share walked
    # lengths with the narrower undersleeve, including the elbow bend.
    wrist_y=h-(612 if outer else 599)
    lower_front,fc0,fc1=split_walk((fb,under_y),shoulder,front_arm,fu/1.02)
    add("front","front",[("hem",(0,0)),("side",(fw,0)),("front_low",(fb,under_y)),("front_high",lower_front),
        ("shoulder",shoulder),("neck",fneck),("opening",(0,cf_y))],
        curves={"side":[(fw,72),(fb-9,under_y-54)],"front_low":fc0,"front_high":fc1,"neck":neck_curve},mirror=True)
    front_panel=pieces[-1]
    sampler_path=Path(__file__).resolve().parents[1]/"public/wardrobe/hussar/braid-sampler.json"
    motif=json.loads(sampler_path.read_text())["pieces"][0]["decor"][0]["pts"]
    front_panel["decor"]=[]
    front_panel["internal"]=[]
    front_panel["note"]="Cut mirrored in shell and lining. Transfer the twelve cord paths before assembly; button the fastening front."
    for row in range(12):
        y=74+row*25.8
        span=(fw*.59)+(row/11)*(fw*.17)
        offset=25 if outer else 7
        cord=[[-offset-(u-5)/87*span,y+(v-18)*.78+(u-5)/87*(5+row*.75)] for u,v in motif]
        front_panel["decor"].append({"kind":"soutache","width_mm":3,"closed":False,"pts":cord})
        button_x,button_y=cord[7]
        front_panel["internal"].append({"kind":"draw","label":"Button station" if row==0 else "","pts":[[button_x-2,button_y],[button_x+2,button_y]]})
    side_walk=length(side_outer,side_inner,side_curve)
    lower_back,sc0,sc1=split_walk(side_outer,side_inner,side_curve,bu/1.02)
    shifted=lambda row:[(x+side_shift,y) for x,y in row]
    side_nodes[2:3]=[("rear_low",(sw+side_dx,under_y)),("rear_high",shifted([lower_back])[0])]
    add("side","side back",side_nodes,curves={"side":[(sw,72),(sw+side_dx-9,under_y-54)],"rear_low":shifted(sc0),"rear_high":shifted(sc1),
        "back_join":[(side_shift+9,172),(side_shift+4,85)]})
    rear_mid,rc0,rc1=split_walk((ba,24),peak,upper(ba,h,False),side_walk*1.02-bu)
    upper_nodes=[("cuff",(-68,wrist_y)),("elbow",(78,wrist_y+6)),("rear_low",(ba,24)),("rear_high",rear_mid),
                 ("front_cap",peak),("forearm",(-fa,24))]
    upper_curves={"rear_low":rc0,"rear_high":rc1,"front_cap":upper(fa,h,True)[::-1],
                  "forearm":[(-fa+2,-138),(-90,wrist_y+90)],"elbow":[(90,wrist_y+114),(ba+5,-170)]}
    add("topsleeve","upper sleeve",upper_nodes,curves=upper_curves,mirror=True,
        note="Ease the cap into the armscye. Match front, side-back and shoulder marks.")
    elbow=length(upper_nodes[1][1],(ba,24),upper_curves["elbow"])
    forearm=length((-fa,24),upper_nodes[0][1],upper_curves["forearm"])
    # Lower sleeve behind the arm, with a hollow cap from front seam to elbow seam.
    cuff1=(-37,24-math.sqrt(forearm**2-(w-37)**2))
    cuff2=(42,24-math.sqrt(elbow**2-(w-42)**2))
    add("undersleeve","under sleeve",[("cuff",cuff1),("elbow",cuff2),("rear_hollow",ub),
        ("front_hollow",trough),("forearm",uf)],curves={"rear_hollow":cub,"front_hollow":cuf})
    cw=length(upper_nodes[0][1],upper_nodes[1][1])+length(cuff1,cuff2)
    cuff_material="black lamb trim" if outer else ("yellow broadcloth" if regiment=="yellow" else cloth)
    add("cuff","pointed cuff", [("wrist_top",(0,0)),("wrist_under",(math.dist(upper_nodes[0][1],upper_nodes[1][1]),0)),("end",(cw,0)),("upper_rear",(cw,53)),
        ("point",(cw*.55,91)),("upper_front",(cw*.39,59)),("start",(0,53))],
        curves={"upper_rear":[(cw*.88,53),(cw*.65,84)],"upper_front":[(cw*.26,54),(cw*.10,53)]},
        material=cuff_material, note="Cut a pair plus facing. Close short ends; apply at wrist.")
    ch=43 if outer else 51
    add("collar","stand collar",[("left_front",(0,0)),("left_back",(fw_neck,0)),("right_back",(collar/2,0)),
        ("right_front",(collar-fw_neck,0)),("end",(collar,0)),("top",(collar,ch)),("start",(0,ch))],1,
        note="Cut one outer and one undercollar. Match shoulders and centre back before setting the stand.")
    pieces[-1]["internal"]=[{"kind":"draw","label":label,"pts":[[x,2],[x,10]]}
        for x,label in [(fw_neck,"Shoulder"),(collar/2,"Centre back"),(collar-fw_neck,"Shoulder")]]
    add("facing","front facing",[("hem",(0,0)),("inner",(48,0)),("neck",(43,cf_y+14)),("opening",(0,cf_y))],
        material=cloth,note="Cut a pair. Sew at the front opening, turn and secure to lining.")
    if outer:
        pieces.append(rectangle("pelisse_front_fur","Pelisse fur opening strip",45,cf_y,2,"black lamb",
                                "Place over the finished opening. Tack flat with nap toward hem. Meet hem trim at lower corners."))
        pieces.append(rectangle("pelisse_hem_fur","Pelisse fur hem half",fw+sw-side_shift+bw,43,2,"black lamb",
                                "Join halves at centre back. Turn to fur side."))
    for side in ("L","R"):
        def pair(tag,p,q,**options): join(f"{tag}_{side}",p.replace("@","."+side),q.replace("@","."+side),**options)
        pair("side","front@/side","side@/side")
        pair("side_back","side@/back_join","back@/side_back")
        pair("shoulder","front@/shoulder","back@/shoulder")
        pair("forearm","topsleeve@/forearm","undersleeve@/forearm")
        pair("elbow","topsleeve@/elbow","undersleeve@/elbow")
        pair("facing","front@/opening","facing@/opening")
        for tag,sleeve,hole in [("cap_front_low","undersleeve@/front_hollow","front@/front_low"),
                ("cap_front_high","topsleeve@/front_cap","front@/front_high"),
                ("cap_rear_low","undersleeve@/rear_hollow","side@/rear_low"),
                ("cap_rear_mid","topsleeve@/rear_low","side@/rear_high"),
                ("cap_rear_high","topsleeve@/rear_high","back@/armscye")]:
            pair(tag,sleeve,hole,kind="ease",ratio=1.02)
        pair("wrist_upper","topsleeve@/cuff","cuff@/wrist_top")
        pair("wrist_under","undersleeve@/cuff","cuff@/wrist_under")
        pair("cuff_closed","cuff@/start","cuff@/end")
        for site in ("front","back"):
            pair("collar_"+site,"collar/"+("left_" if side=="L" else "right_")+site,site+"@/neck")
    for item in pieces:
        marks={
            "front":{"front_high":"front_cap","neck":"front_neck"},
            "side":{"rear_high":"rear_mid"},"back":{"armscye":"rear_cap","neck":"back_neck"},
            "topsleeve":{"front_cap":"front_cap","rear_low":"rear_mid","rear_high":"rear_cap"},
            "collar":{"left_front":"front_neck","right_front":"front_neck","left_back":"back_neck","right_back":"back_neck"},
        }.get(item["id"][len(prefix)+1:],{})
        if marks:item["notches"]=[{"id":edge+"_match","edge":edge,"s":.5,"type":"double" if "rear" in tag or "back" in tag else "V"}
                                 for edge,tag in marks.items()]
    join("center_back","back.R/center","back.L/center")
    checks={"front cap ease":(walk(fa,h,True)+fu)/front_walk,"rear cap ease":(walk(ba,h,False)+bu)/back_walk,
            "collar seam mm":collar,"sleeve cuff mm":cw}
    for k in ("front cap ease","rear cap ease"):
        if abs(checks[k]-1.02)>.001: raise ValueError(checks)
    return pieces,seams,checks


def breeches(regiment):
    pieces,seams=[],[]
    leather="buckskin"
    # Waist to knee follows the adult model's 1070 mm waist and 498 mm knee.
    # The raised centre back, deeper fork and extra seating room are on the back.
    hip=(-20,404)
    lower_curve=[(-20,279),(0,85)]
    lower_length=length(hip,(0,0),lower_curve)
    knee_open,leg_side,vent=split_walk(hip,(0,0),lower_curve,lower_length-66)
    outside=[("hip_side",(0,574)),("leg_side",hip),("knee_open",knee_open)]
    front=[("hem",(0,0)),("inside",(171,0)),("fork",(260,298)),
           ("rise",(230,377)),("fall_top",(222,574)),("waist",(120,574)),*outside]
    inside=[(171,116),(244,252)]
    front_curves={"inside":inside,"fork":[(237,305),(229,339)],"rise":[(230,449),(225,510)],
                  "hip_side":[(0,535),(-20,444)],"leg_side":leg_side,"knee_open":vent}
    pieces.append(panel("breech_front","Breeches front",front,2,leather,
        "Close centre front. Staystitch and face each marked 172 mm fall slit. Leave outer knee vents open.",front_curves,mirror=True))
    pieces[-1]["internal"]=[{"kind":"cut","label":"Fall slit - face before clipping","pts":[[-120,574],[-120,402]]}]
    back=[("hem",(0,0)),("inside",(193,0)),("fork",(282,298)),("seat",(261,394)),
          ("waist",(238,594)),*outside]
    back_curves={"inside":[(193,116),(266,252)],"fork":[(259,308),(245,337)],
                 "seat":[(264,451),(238,548)],"waist":[(174,594),(48,574)],
                 "hip_side":front_curves["hip_side"],"leg_side":leg_side,"knee_open":vent}
    pieces.append(panel("breech_back","Breeches back",back,2,leather,
        "Cut mirror pair. Seat curve rises at centre back. Check seated depth in a toile.",back_curves))
    back_waist=length((238,594),(0,574),back_curves["waist"])
    band_edges=[("extension",32),("right_fall",102),("right_front",120),("right_back",back_waist),
                ("left_back",back_waist),("left_front",120),("left_fall",102)]
    x=0; band=[]
    for label,width in band_edges:
        band.append((label,(x,0)));x+=width
    pieces.append(panel("breech_band","Breeches waistband",band+[("end",(x,0)),("top",(x,70)),("start",(0,70))],1,leather,
        "Fold along centre. Leave both 102 mm centre-front sections independent behind the fall. Button the 32 mm extension."))
    pieces.append(panel("fall","Breeches fall facing",[("base",(-110,0)),("side_r",(110,0)),("right",(102,172)),
        ("left",(0,172)),("side_l",(-102,172))],1,"linen","Face the integral drop front; stitch at the waist and along both slit edges. Turn and button to the band."))
    pieces.append(rectangle("fall_stay","Fall opening stay",37,183,2,"linen","Stay each marked slit. Clip to the lower end and turn; reinforce the corner."))
    pieces.append(panel("knee_band","Breeches knee band",[("fore",(0,0)),("aft",(171,0)),("extension",(364,0)),
        ("end",(389,0)),("fold",(389,36)),("start",(0,36))],2,leather,
        "Fold lengthwise. Match the 171 mm front and 193 mm back; leave 25 mm for the button overlap."))
    pieces.append(rectangle("knee_placket","Breeches knee vent underlap",22,66,2,leather,
                            "Apply to the back edge of the 66 mm knee vent. Finish the opposite vent edge."))
    for side in ("L","R"):
        for seam in ("hip_side","leg_side","inside"):
            stitch(seams,f"breech_{seam}_{side}",f"breech_front.{side}/{seam}",f"breech_back.{side}/{seam}")
        name="right" if side=="R" else "left"
        for pid in ("front","back"):
            stitch(seams,f"band_{pid}_{side}",f"breech_band/{name}_{pid}",f"breech_{pid}.{side}/waist")
        stitch(seams,"fall_waist_"+side,f"fall/{name}",f"breech_front.{side}/fall_top")
        stitch(seams,"knee_front_"+side,f"knee_band.{side}/fore",f"breech_front.{side}/hem")
        stitch(seams,"knee_back_"+side,f"knee_band.{side}/aft",f"breech_back.{side}/hem")
        stitch(seams,"knee_underlap_"+side,f"knee_placket.{side}/right",f"breech_back.{side}/knee_open")
    for pid,edge in [("breech_front","fork"),("breech_front","rise"),("breech_back","fork"),("breech_back","seat")]:
        stitch(seams,pid+"_"+edge,pid+".R/"+edge,pid+".L/"+edge)
    # Fitted over-leggings extend from high thigh to the ankle, under the boot.
    # Draft both side seams from the same profile; the open outseam permits dressing.
    material="navy broadcloth" if regiment=="blue" else "black broadcloth"
    for which,width in [("front",128),("back",143)]:
        top=791
        nodes=[("ankle",(-width/2,0)),("calf_outer",(width/2,0)),("knee_outer",(width/2+34,302)),
               ("thigh_outer",(width/2+26,453)),("scallop1",(width/2+77,top)),
               ("scallop2",(0,top-18)),("thigh_inner",(-width/2-77,top)),
               ("knee_inner",(-width/2-26,453)),("calf_inner",(-width/2-34,302))]
        curves={"scallop1":[(width*.67,top),(29,top+11)],
                "scallop2":[(-28,top+11),(-width*.67,top+1)],
                "calf_outer":[(width/2,70),(width/2+41,215)],
                "calf_inner":[(-width/2-41,215),(-width/2,70)],
                "thigh_outer":[(width/2+37,550),(width/2+77,713)],
                "thigh_inner":[(-width/2-77,713),(-width/2-37,550)],
                "knee_outer":[(width/2+41,352),(width/2+27,415)],
                "knee_inner":[(-width/2-27,415),(-width/2-41,352)]}
        pieces.append(panel("leg_"+which,"Charivades "+which,nodes,2,material,
            "Face scallops with regimental binding. Leave outer seam below knee open for buttons.",curves,mirror=which=="front"))
    ca=length((width/2,0),(width/2+34,302),curves["calf_outer"])
    kn=length((width/2+34,302),(width/2+26,453),curves["knee_outer"])
    pieces.append(panel("leg_placket","Charivades button stand",[("bottom",(0,0)),("calf",(31,0)),
        ("knee",(31,ca)),("top",(31,ca+kn)),("fold",(0,ca+kn))],2,material,
        "Join the divided edge to the back opening, matching the calf station. Fold and set buttons opposite the front."))
    for side in ("L","R"):
        for edge in ("calf_inner","knee_inner","thigh_inner","thigh_outer"):
            stitch(seams,"leg_"+edge+side,"leg_front."+side+"/"+edge,"leg_back."+side+"/"+edge)
        for region in ("calf","knee"):
            stitch(seams,"stand_"+region+side,f"leg_placket.{side}/{region}",f"leg_back.{side}/{region}_outer",reverse=False)
    return pieces,seams


def generate(regiment, out, ftx):
    pieces,seams=breeches(regiment)
    for pelisse in (False,True):
        coat,joins,_=coats(regiment,pelisse)
        pieces.extend(coat)
        seams.extend(joins)
    extras,joins=accessories(regiment)
    pieces.extend(extras)
    seams.extend(joins)
    title=f"Swedish {regiment.title()} Hussars"
    doc={"schema":"fabric-pattern/1","units":"mm","style":"HUSSAR_"+regiment.upper(),
         "meta":{"title":title,"size_label":"178 cm / C100 W86 H58",
                 "sa_mm":10,"origin":"VECTOR_EDIT","note":"Original reconstruction after Schützercrantz. Synthetic adult: stature 178 cm, chest 100 cm, waist 86 cm, head 58 cm. Make a toile. Print at 100%."},
         "pieces":pieces,"seams":seams}
    name="hussar-"+regiment
    pattern=out/(name+".json")
    with pattern.open("x") as f: json.dump(doc,f,indent=1)
    report=json.loads((Path(__file__).resolve().parents[1]/"public/fitted-bodies/hussar.json").read_text())
    body={"schema":"fabric-measurements/1","wearer":{"label":"Synthetic hussar","stature_mm":1780},
          "fused":{k:{"value_mm":v["target_mm"]} for k,v in report["native_tapes"].items()}}
    with tempfile.TemporaryDirectory() as temp:
        scratch=Path(temp)
        measures=scratch/"measures.json"
        measures.write_text(json.dumps(body))
        points=[]
        for jacket,neck,waist in [("dolman",[15,22,32],[32,42,58]),("pelisse",[32,42,57],[75,94,118])]:
            points.extend([
                {"id":jacket+"_neck","label":jacket.title()+" collar setting","kind":"path","body":"neck_base",
                 "refs":[jacket+"_collar/left_front+left_back+right_back+right_front"],"target_mm":neck},
                {"id":jacket+"_waist","label":jacket.title()+" waist at hem","kind":"path","body":"waist",
                 "refs":[f"{jacket}_{pid}.{side}/hem" for pid in ("front","side","back") for side in ("R","L")],"target_mm":waist},
            ])
        hat="mirliton" if regiment=="blue" else "busby"
        points.append({"id":"hat_base","kind":"path","body":"head","refs":[hat+"_sweatband/base0+base1+base2+base3"],"target_mm":[6,10,15]})
        fit=scratch/"fit.json"
        fit.write_text(json.dumps({"schema":"fabric-fit/1","garment":title,"points":points}))
        def native(*args):
            p=subprocess.run([ftx,*map(str,args),"--json"],text=True,capture_output=True)
            data=json.loads(p.stdout)
            if not data["ok"]: raise ValueError(data)
            return data["result"]
        validation=native("validate",pattern,"--measurements",measures,"--fit",fit)
        bad=[s for s in validation["seams"] if s["status"]!="PASS"]
        if bad or validation.get("structure_errors"): raise ValueError((bad,validation.get("structure_errors")))
        native("export","a4",pattern,"--out",scratch/"pages","--layout-width","1100","--created","2026-11-19T00:00")
        check=native("check",scratch/"pages","--pattern-ref",pattern)
        pdf,=(scratch/"pages").glob("*_A4.pdf")
        with (out/(name+".pdf")).open("xb") as f, pdf.open("rb") as src: shutil.copyfileobj(src,f)
        print(title,len(pieces),"templates,",len(seams),"seams,",len(validation["truing"]),"truing checks;",
              [(p["id"],p["finished_mm"]) for p in validation["points"]],"; PDF",check["passed"],"/",check["total"])


if __name__=="__main__":
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument("output",type=Path)
    cli.add_argument("--ftx",type=Path,default=Path(__file__).resolve().parents[2]/"target/release/ftx")
    args=cli.parse_args()
    args.output.mkdir(parents=True,exist_ok=True)
    for variant in ("blue","yellow"): generate(variant,args.output,args.ftx.resolve())
