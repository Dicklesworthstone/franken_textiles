"""Draft, solve and export the six-gore skirt study using FrankenTextiles. Run from the repository root."""
import argparse
import gzip
import hashlib
import json
import math
import struct
import subprocess
from pathlib import Path
from runpy import run_path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
FTX = ROOT/"target/release/ftx"
HERE = Path(__file__).resolve().parent
adult = run_path(str(HERE/"prepare-sundress.py"))
replay = run_path(str(HERE/"prepare-drape-room.py"))
CUTS = {"column": 1260, "flare": 1680}
WAISTS = [650, 680, 700, 710, 740, 800]


def pattern(cut):
    hem = CUTS[cut]
    pieces, joins = [], []
    for i, label in enumerate(["Front", "Front right", "Back right", "Back", "Back left", "Front left"]):
        # Sewing lines. Each gore shapes the hip 18 cm below the waist.
        # Keep the physical right edge of one gore paired with the left edge on its neighbour.
        n = f"gore_{i+1}"
        nodes = [("hl",-hem/12,-590),("hr",hem/12,-590),("pr",1044/12,-180),
                 ("wr",780/12,0),("wl",-780/12,0),("pl",-1044/12,-180)]
        def controls(a,b,p,q):
            dx,dy=b[0]-a[0],b[1]-a[1]
            return [[((x-a[0])*dx+(y-a[1])*dy)/(dx*dx+dy*dy),
                     ((y-a[1])*dx-(x-a[0])*dy)/(dx*dx+dy*dy)] for x,y in (p,q)]
        # The side sewing lines meet both the waist and hem at right angles when joined.
        h=hem/12
        lines=[("hem",None),
               ("right_lower",controls((h,-590),(87,-180),(h,-530),(92,-240))),
               ("right_upper",controls((87,-180),(65,0),(82,-120),(65,-58))),
               ("waist",None),
               ("left_upper",controls((-65,0),(-87,-180),(-65,-58),(-82,-120))),
               ("left_lower",controls((-87,-180),(-h,-590),(-92,-240),(-h,-530)))]
        piece = adult["panel"](n,label+" gore",i*60,nodes,lines,[[0,-490],[0,-95]],
                               "Join the marked gore edges. Leave an opening in the left side seam for a zip. Finish the waist with binding.")
        piece["placement"]["radius_mm"] = round(hem/(2*math.pi)+10,2)
        pieces.append(piece)
        next_piece=f"gore_{(i+1)%6+1}"
        joins.append({"id":f"join_{i+1}", "a":n+"/right_lower+right_upper",
                      "b":next_piece+"/left_upper+left_lower","reverse":True})
    return {"schema":"fabric-pattern/1","units":"mm","style":"SIX_GORE_"+cut.upper(),
            "meta":{"title":"Six-gore skirt — "+cut,"size_label":"Synthetic adult study","sa_mm":10,
                    "note":"Six panels. Bound waist and a side zip. Allow for hems and test the cut in a muslin."},
            "pieces":pieces,"seams":joins}


def inputs(folder):
    folder.mkdir(parents=True,exist_ok=False)
    for cut in CUTS:
        (folder/(cut+".json")).write_text(json.dumps(pattern(cut),indent=2)+"\n")
    for w in WAISTS:
        doc=adult["body"](waist=w,hip=w+240)
        (folder/f"form-{w}.json").write_text(json.dumps(doc,indent=2)+"\n")
    # Native flat and draped readings are distinct. A path along the six waist edges is a flat girth.
    fit={"schema":"fabric-fit/1","garment":"Six-gore skirt","points":[
        {"id":"waist","label":"Finished waist","kind":"path","refs":[f"gore_{i}/waist" for i in range(1,7)],
         "body":"waist","target_mm":[10,40,100]}]}
    (folder/"fit.json").write_text(json.dumps(fit,indent=2)+"\n")


def solve(folder,cut,w):
    name=f"{cut}-{w}"
    result=subprocess.run([FTX,"drape",folder/(cut+".json"),"--measurements",folder/f"form-{w}.json",
        "--fit",folder/"fit.json","--arm-deg","45","--spacing-mm","25","--self-contact-mm","1",
        "--sew","16","--drape","48","--out",folder/name,"--json"],capture_output=True)
    (folder/(name+"-run.json")).write_bytes(result.stdout)
    (folder/(name+".log")).write_bytes(result.stderr)
    if not result.stdout:raise RuntimeError(result.stderr.decode())
    print(name,json.loads(result.stdout).get("refusal",{}).get("message","completed"))


def display_form(body, faces):
    """Trim a section of the run's fitted skin into a tailor's bust, closing the cut planes."""
    sections = [(2,1,.57),(2,-1,.012),(0,1,.163),(0,-1,.163)]
    vertices, triangles, lookup = [], [], {}
    bounds = [[] for _ in sections]
    def index(p):
        key=tuple(np.round(p,6))
        if key not in lookup:
            lookup[key]=len(vertices)
            vertices.append(p)
        return lookup[key]
    for face in faces:
        poly=list(body[face])
        if any(all(p[axis]*sign>limit for p in poly) for axis,sign,limit in sections):
            continue
        for axis,sign,limit in sections:
            clipped=[]
            for a,b in zip(poly,poly[1:]+poly[:1]):
                da,db=a[axis]*sign-limit,b[axis]*sign-limit
                if da<=0:clipped.append(a)
                if (da<0)!=(db<0):clipped.append(a+(b-a)*(da/(da-db)))
            poly=clipped
        if len(poly)<3:continue
        ids=list(map(index,poly))
        for j in range(1,len(ids)-1):triangles.extend([ids[0],ids[j],ids[j+1]])
        for k,(axis,sign,limit) in enumerate(sections):
            for a,b in zip(ids,ids[1:]+ids[:1]):
                if a!=b and max(abs(vertices[i][axis]*sign-limit) for i in (a,b))<1e-7:
                    bounds[k].append((a,b))
    cap_start=len(triangles)
    # Keep each armhole separate from any hip cut made by the same lateral plane.
    for edges in bounds:
        adjacent={}
        for a,b in edges:
            adjacent.setdefault(a,[]).append(b);adjacent.setdefault(b,[]).append(a)
        while adjacent:
            queue=[next(iter(adjacent))];ring=set()
            while queue:
                i=queue.pop()
                if i in ring:continue
                ring.add(i);queue.extend(adjacent.pop(i,[]))
            rim={i:len(vertices)+j for j,i in enumerate(sorted(ring))}
            vertices.extend([vertices[i] for i in sorted(ring)])
            center=len(vertices);vertices.append(np.mean([vertices[i] for i in ring],axis=0))
            for a,b in edges:
                if a in ring:triangles.extend([center,rim[b],rim[a]])
    # Princess seams follow the exact triangle surface along radial cutting planes.
    surface=np.asarray(vertices)[np.asarray(triangles[:cap_start]).reshape(-1,3)]
    stitches=[]
    for theta in np.radians([30,-30,146,-146]):
        direction=np.array([math.sin(theta),-math.cos(theta)])
        normal=np.array([-direction[1],direction[0]])
        center=np.array([0,-.09])
        s=(surface[:,:,:2]-center)@normal
        line=[]
        for triangle,distance in zip(surface[(s.min(1)<0)&(s.max(1)>0)],s[(s.min(1)<0)&(s.max(1)>0)]):
            pair=[]
            for i in range(3):
                j=(i+1)%3
                if (distance[i]<0)==(distance[j]<0):continue
                p=triangle[i]+(triangle[j]-triangle[i])*(distance[i]/(distance[i]-distance[j]))
                if np.dot(p[:2]-center,direction)>0:
                    p[:2]+=direction*.00045
                    pair.append(p)
            if len(pair)==2:line.extend(pair)
        stitches.extend(line)
    # Keep the real pelvis and legs below the ribs for the garment-detail camera.
    lower=faces[(body[faces][:,:,2].min(1)<.36)&(np.abs(body[faces][:,:,0]).max(1)<.235)]
    used,remap=np.unique(lower,return_inverse=True)
    return {"lowerXYZ":np.rint(body[used]*10000).astype(int).flatten().tolist(),"lowerTri":remap.flatten().tolist(),
            "xyz":np.rint(np.asarray(vertices)*10000).astype(int).flatten().tolist(),
            "tri":triangles,"capStart":cap_start,"stitches":np.rint(np.asarray(stitches)*10000).astype(int).flatten().tolist()}


def compact(data):
    return (json.dumps(data,separators=(",",":"),allow_nan=False)+"\n").encode()


def package(folder,output):
    output.mkdir(exist_ok=True,parents=True)
    summary={"runs":{},"refusals":{},"cuts":CUTS}
    read=replay["read_obj"]
    verify=run_path(str(HERE/"prepare-study-patterns.py"))["verify_paper"]
    for cut in CUTS:
        pat=folder/(cut+".json")
        target=folder/("print-"+cut)
        if not target.exists():
            subprocess.run([FTX,"export","a4",pat,"--out",target,"--created","2026-11-19T00:00:00Z","--json"],check=True,stdout=subprocess.DEVNULL)
        subprocess.run([FTX,"check",target,"--pattern-ref",pat],check=True)
        pdf,=target.glob("*.pdf")
        verify(pdf)
        (output/(cut+".pdf")).write_bytes(pdf.read_bytes())
        (output/(cut+".json")).write_bytes(pat.read_bytes())
        topology=None
        for w in WAISTS:
            key=f"{cut}-{w}"
            result=folder/(key+"-run.json")
            if not result.exists() or not result.stat().st_size:continue
            response=json.loads(result.read_text())
            if "refusal" in response:
                summary["refusals"][key]=response["refusal"]["message"].split("; diagnostics")[0]
                continue
            d=folder/key
            r=json.loads((d/"sim_report.json").read_text())
            m=json.loads((d/"mesh_map.json").read_text())
            cache=(d/"fold.pc2").read_bytes()
            magic,version,n,_,_,nf=struct.unpack_from("<12siiffi",cache)
            assert magic==b"POINTCACHE2\x00" and version==1 and nf==sum(m["frames"].values())
            xyz=np.frombuffer(cache,dtype="<f4",offset=32).reshape(nf,n,3)
            colored,tri=read(d/"garment_strain.obj")
            assert np.array_equal(colored[:,:3].astype("<f4"),xyz[-1]) and pat.read_bytes()==Path(r["pattern"]).read_bytes()
            frames=list(range(nf-m["frames"]["drape"]-m["frames"]["sew"],nf-1,4))+[nf-1]
            uv=np.zeros((n,2))
            for a,b in m["inst_range"].values():
                uv[a:b]=xyz[0,a:b][:,[0,2]]-xyz[0,a:b][:,[0,2]].min(axis=0)
            top={"tri":tri.flatten().tolist(),"uvMm":np.rint(uv*1000).astype(int).flatten().tolist(),
                 "panels":list(m["inst_range"].values()),"seams":[s["side_a"] for s in m["seams"].values()],
                 "joins":[{"side_a":s["side_a"],"side_b":s["side_b"]} for s in m["seams"].values()],
                 "waist":[v for k,v in m["edges"].items() if k.endswith("/waist")],
                 "hem":[v for k,v in m["edges"].items() if k.endswith("/hem")],"frames":frames}
            assert topology is None or top==topology
            topology=top
            (output/(cut+"-mesh.json")).write_bytes(compact(top))
            payload=replay["quantize"](xyz[frames])+np.rint(255*colored[:,3:]).astype("u1").tobytes()
            (output/(key+".gz")).write_bytes(gzip.compress(payload,mtime=0))
            bust=display_form(*read(d/"body.obj"))
            body_file=output/f"bust-{w}.json.gz"
            body_data=gzip.compress(compact(bust),mtime=0)
            if cut=="flare" and f"column-{w}" in summary["runs"]:
                assert body_file.read_bytes()==body_data
            body_file.write_bytes(body_data)
            outline=[xyz[-1,v] for i,path in enumerate(top["hem"]) for v in path[:-1]]
            summary["runs"][key]={"waist":w,"hip":w+240,"tape":r["body"]["fit"]["tape"],"strain":r["global_strain_pct"],
                "hemOutline":np.rint(np.asarray(outline)*10000).astype(int).flatten().tolist(),
                "hemHeights":r["hem_heights_mm"],"hemRise":r["hem_rise_mm"],"flags":r["gating"],"settled":r["numerical"]["settled"],"drape":m["frames"]["drape"],
                "sew":m["frames"]["sew"],"fabric":r["fabric"],"selfContact":r["self_contact"]["enabled"],
                "pc2Sha256":hashlib.sha256(cache).hexdigest(),"patternSha256":hashlib.sha256(pat.read_bytes()).hexdigest()}
    (output/"study.json").write_bytes(compact(summary))


if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("folder",type=Path)
    p.add_argument("--inputs",action="store_true")
    p.add_argument("--solve",nargs=2,metavar=("CUT","WAIST_MM"))
    p.add_argument("--package",type=Path)
    args=p.parse_args()
    if args.inputs:inputs(args.folder)
    if args.solve:solve(args.folder,args.solve[0],int(args.solve[1]))
    if args.package:package(args.folder,args.package)
