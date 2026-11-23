"""Render the cutting, assembly and dressed views with Blender Cycles, then encode with FFmpeg.

blender -b -t 8 -P showcase/scripts/render-construction.py -- sundress NEW_DIR
For bolero, trousers, ensemble, column and flare, append the matching native drape directory.
--proof renders the three chapter stills. Omit it for the motion study.
"""
import argparse
import gzip
import hashlib
import json
import math
import os
import struct
import subprocess
import sys
from pathlib import Path

import numpy as np

try:
    import bpy
    from mathutils import Vector
except ModuleNotFoundError:
    # The archived geometry and edit are also shared with the headless raster recorder.
    bpy = None

PUBLIC = Path(__file__).resolve().parents[1] / "public"
FPS = 60
COUNT = 6 * FPS
FRAME_WIDTH, FRAME_HEIGHT = 3840, 2160
LANDMARKS = (0, 53, 90)
FABRIC_COLORS = {
    "sundress": (.16, .38, .29), "eased": (.16, .38, .29),
    "blue": (.013, .022, .049), "yellow": (.019, .019, .022),
    "trousers": (.052, .103, .137), "bolero": (.27, .042, .028),
    "bolero-full-hand": (.55, .137, .068),
    "ensemble": (.03, .14, .105), "fez": (.26, .017, .021),
    "column": (.28, .16, .09), "flare": (.32, .15, .07),
}


def document(path):
    return json.loads(path.read_bytes())


def read_obj(path):
    vertices, triangles = [], []
    for row in path.read_text().splitlines():
        f = row.split()
        if f[:1] == ["v"]:
            vertices.append(tuple(map(float, f[1:4])))
        elif f[:1] == ["f"]:
            triangles.append(tuple(int(i.split("/")[0])-1 for i in f[1:4]))
    return np.array(vertices), np.array(triangles)


def fez():
    # Develop the drafted circular sector into a truncated cone, conserving arc and slant lengths.
    spec = document(PUBLIC / "patterns/fez.json")["meta"]["solved"]
    base, top = [spec[k]/(2000*math.pi) for k in ("base_girth_mm", "top_girth_mm")]
    h, slant, sector = spec["height_mm"]/1000, spec["slant_mm"]/1000, math.radians(spec["sector_deg"])
    angles = np.linspace(-math.pi, math.pi, 97)
    faces = []
    for row in range(4):
        for j in range(96):
            i = row*97+j
            faces.extend([(i,i+1,i+98),(i,i+98,i+97)])
    lid = len(angles)*5
    faces.extend((lid,lid+1+j,lid+2+j) for j in range(96))
    ring = np.column_stack((top*np.sin(angles),-top*np.cos(angles),np.zeros_like(angles)))
    ring[-1] = ring[0]
    frames = []
    for t in np.linspace(0,1,71):
        a = sector+(2*math.pi-sector)*t
        rows = []
        for k in np.linspace(0,1,5):
            r = (base+(top-base)*k)*2*math.pi/a
            theta = angles*a/(2*math.pi)
            # A planar strip at the start, a circular frustum when the back seam has closed.
            cu = np.sin(theta)*r
            cv = -np.cos(theta)*r
            slope = math.acos(min(1,(base-top)*2*math.pi/(a*slant)))
            height = k*slant*math.sin(slope)
            turn = (1-t)*math.pi/2
            rows.append(np.column_stack((cu,cv*math.cos(turn)-height*math.sin(turn),
                cv*math.sin(turn)+height*math.cos(turn))))
        cap = np.vstack(([0,0,0],ring))
        turn = (1-t)*math.pi/2
        y = cap[:,1].copy()
        cap[:,1] = y*math.cos(turn)
        cap[:,2] = y*math.sin(turn)+h+(1-t)*.035
        cap[:,0] += (1-t)*.31
        if t == 1:
            for row in rows:
                row[-1] = row[0]
            cap[1:] = rows[-1]
            cap[0,2] = rows[-1][0,2]
        frames.append(np.vstack([*rows,cap]))
    return np.array(frames),np.arange(71),np.array(faces),{"crown":(0,lid),"tip":(lid,lid+98)},None,None,{},{}


def load(name, native):
    if name == "fez":
        return fez()
    packed = {"sundress":"sundress/dress", "eased":"sundress/light", "blue":"hussar/dolman", "yellow":"hussar/dolman"}
    if name in packed:
        source = PUBLIC / "exhibits" / packed[name]
        layout = document(source.with_suffix(".json"))
        data = gzip.decompress(source.with_suffix(".gz").read_bytes())
        nb,ni,n,nf = struct.unpack_from("<4I",data)
        body = np.frombuffer(data,"<f4",nb*3,16).reshape(-1,3)
        indices = np.frombuffer(data,"<u4",ni,16+nb*12).reshape(-1,3)
        frames = np.frombuffer(data,"<f4",offset=16+nb*12+ni*4+n*8).reshape(nf,n,3)
        return frames,np.array(layout["frames"]),np.array(layout["triangles"]).reshape(-1,3),layout["panels"],body,indices,layout,layout["phases"]
    mesh = document(native/"mesh_map.json")
    pointcache = (native/"fold.pc2").read_bytes()
    if name in ("column","flare"):
        expected = document(PUBLIC/"atelier/study.json")["runs"][name+"-680"]["pc2Sha256"]
    elif name == "bolero":
        expected = document(PUBLIC/"exhibits/poses/runs.json")["cases"]["520-35"]["sourcePc2Sha256"]
    elif name == "bolero-full-hand":
        # The admitted 355 mm cut from the visual fitting walkthrough.
        expected = "ca9e425831e0b8ec3f4f11e51d0974226d3b63ab115f4420dde54ab74c7b478a"
    elif name == "ensemble":
        expected = document(PUBLIC/"exhibits/poses/ensemble.json")["pc2Sha256"]
    else:
        expected = next(c["pc2Sha256"] for c in document(PUBLIC/"exhibits/trouser-study/study.json")["cases"] if c["id"]=="standing")
    if hashlib.sha256(pointcache).hexdigest() != expected:
        raise ValueError("The film must show the published run")
    magic,version,n,_,_,nf = struct.unpack_from("<12siiffi",pointcache)
    assert magic==b"POINTCACHE2\0" and version==1 and nf==sum(mesh["frames"].values())
    frames = np.frombuffer(pointcache,"<f4",offset=32).reshape(nf,n,3)
    endpoint,tri = read_obj(native/"garment.obj")
    assert np.array_equal(endpoint.astype("<f4"), frames[-1])
    body,indices = read_obj(native/"body.obj")
    return frames,np.arange(nf),tri,mesh["inst_range"],body,indices,mesh,mesh["frames"]


def material(name,color,cloth=False,texture=None,pitch=.19):
    mat=bpy.data.materials.new(name); mat.diffuse_color=(*color,1);mat.use_nodes=True
    nodes=mat.node_tree.nodes; link=mat.node_tree.links.new
    shader=nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value=(*color,1)
    shader.inputs["Roughness"].default_value=.86 if cloth else .65
    if cloth:
        shader.inputs["Sheen Weight"].default_value=.15
        shader.inputs["Sheen Tint"].default_value=(*color,1)
        shader.inputs["Specular IOR Level"].default_value=.16
        uv=nodes.new("ShaderNodeTexCoord"); mapping=nodes.new("ShaderNodeVectorMath")
        mapping.operation="SCALE";mapping.inputs["Scale"].default_value=1/pitch;link(uv.outputs["UV"],mapping.inputs[0])
        if texture:
            image=nodes.new("ShaderNodeTexImage");image.image=bpy.data.images.load(str(PUBLIC/texture),check_existing=True)
            link(mapping.outputs[0],image.inputs[0]); link(image.outputs["Color"],shader.inputs["Base Color"])
        else:
            # Preserve millimetre-scale variation in the yarn even on solid-dyed pieces.
            albedo=nodes.new("ShaderNodeTexImage");albedo.image=bpy.data.images.load(str(PUBLIC/"materials/linen.webp"),check_existing=True)
            link(mapping.outputs[0],albedo.inputs[0])
            tint=nodes.new("ShaderNodeMixRGB");tint.blend_type="MULTIPLY";tint.inputs[0].default_value=.8
            tint.inputs[1].default_value=(*color,1);link(albedo.outputs["Color"],tint.inputs[2]);link(tint.outputs[0],shader.inputs["Base Color"])
        relief=nodes.new("ShaderNodeTexImage");relief.image=bpy.data.images.load(str(PUBLIC/"materials/linen-normal.webp"),check_existing=True)
        relief.image.colorspace_settings.name="Non-Color";link(mapping.outputs[0],relief.inputs[0])
        normal=nodes.new("ShaderNodeNormalMap");normal.inputs["Strength"].default_value=.62
        link(relief.outputs["Color"],normal.inputs["Color"]);link(normal.outputs[0],shader.inputs["Normal"])
    return mat


def surface(name,xyz,tri,mat):
    mesh=bpy.data.meshes.new(name);mesh.from_pydata(xyz.tolist(),[],tri.tolist());mesh.update()
    obj=bpy.data.objects.new(name,mesh);bpy.context.collection.objects.link(obj);mesh.materials.append(mat)
    mesh.polygons.foreach_set("use_smooth",np.ones(len(tri),dtype=bool))
    return obj


def curves(name,paths,material,radius):
    data=bpy.data.curves.new(name,"CURVE");data.dimensions="3D";data.resolution_u=1;data.bevel_depth=radius;data.bevel_resolution=1
    obj=bpy.data.objects.new(name,data);bpy.context.collection.objects.link(obj);data.materials.append(material)
    for path in paths:
        spline=data.splines.new("POLY");spline.points.add(len(path)-1)
    return obj


def set_curve(obj,paths):
    for spline,points in zip(obj.data.splines,paths):
        spline.points.foreach_set("co",np.column_stack((points,np.ones(len(points)))).astype("f").ravel())


def ease(t):
    t=np.clip(t,0,1);return t*t*(3-2*t)

def cutting_table(xyz, panels, gap, height):
    """Translate the current shapes into disjoint cells; their extents change as they roll."""
    result = xyz.copy()
    groups = sorted(panels.values())
    cols = math.ceil(len(groups)/2) if len(groups)>3 else len(groups)
    row_top = 0.
    for start in range(0,len(groups),cols):
        row = groups[start:start+cols]
        x = depth = 0.
        for a,b in row:
            lo,hi = xyz[a:b].min(0),xyz[a:b].max(0)
            result[a:b] += [x-lo[0], -(lo[1]+hi[1])/2, row_top-hi[2]]
            x += hi[0]-lo[0]+gap
            depth = max(depth,hi[2]-lo[2])
        for a,b in row:
            result[a:b,0] -= (x-gap)/2
        row_top -= depth+gap
    result[:,2] += height-(row_top+gap)/2
    return result


def film_pose(i, name, frames, ids, phases, panels, gap, height):
    """Panel study, then a shot cut to the admitted sewn state. No in-between solver poses."""
    staging = i < 61
    if name == "fez":
        if staging:
            source = round(np.interp(i,[0,12,60],[0,0,70]))
            return cutting_table(frames[source],panels,gap,height),True
        xyz = frames[-1].copy()
        a,b = panels["tip"]
        xyz[a:b,2] += .035*(1-ease((i-61)/26))
        return xyz,False
    fold,zip_count,sew = phases["fold"],phases["zip"],phases["sew"]
    if staging:
        target = np.interp(i,[0,12,55,60],[0,0,fold-1,fold-1])
        available = np.flatnonzero(ids<fold)
    else:
        start = fold+zip_count-1
        target = np.interp(i,[61,74,89,95],[start,start+sew,ids[-1],ids[-1]])
        available = np.flatnonzero(ids>=start)
    index = available[np.abs(ids[available]-target).argmin()]
    xyz = frames[index]
    return (cutting_table(xyz,panels,gap,height) if staging else xyz), staging


def render(args):
    frames,ids,tri,panels,body,body_tri,layout,phases=load(args.garment,args.native)
    name=args.garment; final=frames[-1]; initial=frames[0]
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene=bpy.context.scene;scene.render.engine=args.engine;scene.cycles.samples=args.samples
    scene.cycles.use_denoising=True;scene.cycles.max_bounces=3;scene.cycles.transparent_max_bounces=2
    if args.engine == "BLENDER_EEVEE_NEXT":
        scene.eevee.taa_render_samples=args.samples
    scene.render.use_persistent_data=True
    scene.render.resolution_x=FRAME_WIDTH;scene.render.resolution_y=FRAME_HEIGHT;scene.render.resolution_percentage=100
    scene.render.image_settings.file_format="JPEG";scene.render.image_settings.quality=93
    scene.world=bpy.data.worlds.new("Studio skylight");scene.world.use_nodes=True
    scene.world.node_tree.nodes["Background"].inputs[0].default_value=(.72,.77,.85,1)
    scene.world.node_tree.nodes["Background"].inputs[1].default_value=.20
    scene.view_settings.look="AgX - Medium High Contrast"
    textile={"sundress":"atelier/madder.webp","eased":"atelier/madder.webp","flare":"atelier/madder.webp","column":"atelier/glen-check.webp"}.get(name)
    fabric=material("Woven face",FABRIC_COLORS[name],True,textile,.16 if name=="column" else .30)
    cloth=surface("Cut pieces",initial,tri,fabric)
    paper=material("Pattern tissue",(.68,.46,.245))
    cloth.data.materials.append(paper)
    flat=initial[:,[0,2]].copy(); loop_idx=np.empty(len(cloth.data.loops),dtype=np.int32)
    cloth.data.loops.foreach_get("vertex_index",loop_idx)
    cloth.data.uv_layers.new().data.foreach_set("uv",flat[loop_idx].astype("f").ravel())
    if name=="ensemble":
        cloth.data.materials.append(material("Trouser twill",(.23,.084,.047),True))
        mask=np.zeros(len(initial),dtype=np.int32)
        for key,(a,b) in panels.items(): mask[a:b]=int(key.startswith("trousers::"))
        mask*=2
    else:
        mask=np.zeros(len(initial),dtype=np.int32)
    form=None;stand=[]
    if body is not None:
        if name in ("blue","yellow","bolero"):
            # Display the jacket on a bust. Keep the solved shoulder, neck, arms and hands.
            hip=min(final[a:b,2].min() for key,(a,b) in panels.items() if "back" in key)-.021
            half_width=.27 if name!="bolero" else .125
            trunk=np.abs(body[:,0])<half_width
            ring=body[trunk & (np.abs(body[:,2]-hip)<.016)]
            body_tri=body_tri[~(trunk & (body[:,2]<hip))[body_tri].any(axis=1)]
            lo,hi=ring.min(0),ring.max(0);center=(lo+hi)/2
            wood=material("Turned walnut",(.079,.044,.023))
            for z,depth,sx,sy in [(hip,.036,(hi[0]-lo[0])*.52,(hi[1]-lo[1])*.55),
                    ((hip+body[:,2].min())/2,hip-body[:,2].min(),.017,.017)]:
                bpy.ops.mesh.primitive_cylinder_add(vertices=80,radius=1,depth=depth,location=(center[0],center[1],z))
                mount=bpy.context.object;mount.scale=(sx,sy,1);mount.data.materials.append(wood);stand.append(mount)
        form=surface("Reference form",body,body_tri,material("Linen covered form",(.28,.18,.105)))
        shrink=form.modifiers.new("Display clearance","DISPLACE");shrink.strength=-.0015;shrink.mid_level=0
        shrink.direction="NORMAL"
    # Use the actual submitted braid paths and their barycentric anchors, including during fold-up.
    brass=material("Couched cord",(.65,.34,.071) if name=="blue" else (.68,.68,.62))
    anchors=[np.array(c["anchors"]).reshape(-1,3) for c in layout.get("cords",[])]
    braid=curves("Chest braid",anchors,brass,.0016) if anchors else None
    buttons=np.array([b["anchor"] for b in layout.get("buttons",[])])
    metal=brass.node_tree.nodes.get("Principled BSDF");metal.inputs["Metallic"].default_value=.48
    studs=[]
    if len(buttons):
        bpy.ops.mesh.primitive_uv_sphere_add(segments=10,ring_count=6,radius=.004)
        base=bpy.context.object;base.data.materials.append(brass);studs.append(base)
        for _ in buttons[1:]:
            o=base.copy();bpy.context.collection.objects.link(o);studs.append(o)
    def carry(xyz,anch,lift):
        faces=xyz[tri[anch[:,0].astype(int)]]
        normals=np.cross(faces[:,1]-faces[:,0],faces[:,2]-faces[:,0])
        normals/=np.maximum(np.linalg.norm(normals,axis=1,keepdims=True),1e-12)
        a,b=anch[:,1:2],anch[:,2:3]
        return a*faces[:,0]+b*faces[:,1]+(1-a-b)*faces[:,2]+normals*lift
    final_mid=(final.min(0)+final.max(0))/2
    size=max(np.ptp(final,axis=0));floor=body[:,2].min() if body is not None else final[:,2].min()
    bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,floor-.002))
    ground=bpy.context.object;ground.data.materials.append(material("Sweep",(.18,.148,.115)))
    def area(title,delta,power,diameter,tint):
        loc=Vector(final_mid)+Vector(delta)*size;l=bpy.data.lights.new(title,"AREA")
        l.energy=power*size**2;l.shape="DISK";l.size=diameter*size;l.color=tint
        o=bpy.data.objects.new(title,l);bpy.context.collection.objects.link(o);o.location=loc
        o.rotation_euler=(Vector(final_mid)-loc).to_track_quat("-Z","Y").to_euler()
    area("Window",(-2.8,-2.1,2.8),370,1.9,(1,.91,.79));area("Rim",(1.7,1.3,2),260,1.45,(1,.93,.80))
    area("Fill",(2,-3,1.5),52,2.8,(.76,.85,1))
    bpy.ops.object.camera_add();cam=bpy.context.object;scene.camera=cam;cam.data.type="ORTHO";cam.data.clip_end=120
    # Compact cutting-table arrangement. Translation only, applied per flat piece at its true scale.
    gap=.045*size
    board=cutting_table(initial,panels,gap,final_mid[2])
    # Ink the sewing line on the actual boundary. Each label names one cut instance.
    edges=np.sort(np.vstack([tri[:,[0,1]],tri[:,[1,2]],tri[:,[2,0]]]),axis=1)
    edges,counts=np.unique(edges,axis=0,return_counts=True);boundary=edges[counts==1]
    ink=material("Pattern ink",(.10,.067,.037))
    outline=curves("Sewing line",boundary,ink,size*.0012)
    set_curve(outline,board[boundary]+[0,-.0013,0])
    labels=[]
    for key,(a,b) in panels.items():
        pts=board[a:b];extent=np.ptp(pts,axis=0);title=key.split("::")[-1].replace("dolman_","").replace("_"," ").replace("."," ").title()
        font=bpy.data.curves.new("Piece name","FONT");font.body=title;font.align_x="CENTER";font.align_y="CENTER"
        font.size=min(extent[0]*1.46/max(3,len(title)),extent[2]*.27,size*.036)
        o=bpy.data.objects.new(title,font);bpy.context.collection.objects.link(o)
        mid=pts.mean(0);mid[1]=pts[:,1].min()-.0025
        o.location=mid;o.rotation_euler.x=math.pi/2;font.materials.append(ink);labels.append(o)
    paths=list(layout.get("seams",{}).values())
    pairs=[]
    for seam in paths:
        a,b=np.asarray(seam["side_a"]),np.asarray(seam["side_b"])
        if not len(a) or not len(b):continue
        if np.linalg.norm(final[a[0]]-final[b[-1]])<np.linalg.norm(final[a[0]]-final[b[0]]):b=b[::-1]
        if len(a)<6:continue
        for t in (.25,.7):pairs.append([a[round(t*(len(a)-1))],b[round(t*(len(b)-1))]])
    thread=curves("Matching edges",pairs,material("Basting thread",(.85,.56,.21)),size*.001) if pairs else None
    def compose(i, destination):
        xyz,staging=film_pose(i,name,frames,ids,phases,panels,gap,final_mid[2])
        cloth.data.vertices.foreach_set("co",xyz.astype("f").ravel())
        cloth.data.polygons.foreach_set("material_index",np.ones(len(tri),dtype=np.int32) if i<=12 else mask[tri[:,0]])
        cloth.data.update();outline.hide_render=i>12
        for label in labels:label.hide_render=i>12
        if braid:set_curve(braid,[carry(xyz,a,.0016) for a in anchors])
        if studs:
            for o,point in zip(studs,carry(xyz,buttons,.003)):o.location=point
        if thread:
            thread.hide_render=not 46<=i<=68;set_curve(thread,xyz[np.asarray(pairs)])
        if form:form.hide_render=staging
        for mount in stand:mount.hide_render=staging
        ground.location.z=min(floor,xyz[:,2].min())-.002
        yaw=.34*ease((i-13)/57)+.32*ease((i-78)/17)
        direction=Vector((math.sin(yaw),-math.cos(yaw),.065+.07*ease((i-20)/50))).normalized()
        rot=(-direction).to_track_quat("-Z","Y")
        right=np.array(rot@Vector((1,0,0)));up=np.array(rot@Vector((0,1,0)))
        u=xyz@right;v=xyz@up;mid=(xyz.min(0)+xyz.max(0))/2
        mid+=right*((u.max()+u.min())*.5-mid@right)+up*((v.max()+v.min())*.5-mid@up)
        cam.location=Vector(mid)+direction*max(3,size*4);cam.rotation_euler=rot.to_euler()
        cam.data.ortho_scale=max(np.ptp(u)*1.22,np.ptp(v)*FRAME_WIDTH/FRAME_HEIGHT*1.20)
        scene.render.filepath=str(destination);bpy.ops.render.render(write_still=True)
    if args.proof:
        for i in LANDMARKS:
            dest=args.out/f"{i:03}.jpg"
            if not dest.is_file():
                compose(i,dest)
        return
    for frame in range(args.start_frame,args.end_frame):
        dest=args.out/f"{frame:03}.jpg"
        if dest.is_file():continue
        # Retain the published six-second edit. Sample camera motion at 60 Hz while
        # snapping cloth geometry to the original recorded states in film_pose.
        i=frame*16/FPS
        if 0<i<=12 and (args.out/"000.jpg").is_file():
            os.link(args.out/"000.jpg",dest)
        else:
            compose(min(i,95),dest)
    if args.start_frame == 0 and args.end_frame == COUNT:
        if name=="fez":
            source="flat pattern "+hashlib.sha256((PUBLIC/"patterns/fez.json").read_bytes()).hexdigest()
        elif args.native:
            source="fold.pc2 "+hashlib.sha256((args.native/"fold.pc2").read_bytes()).hexdigest()
        else:
            source="fold.pc2 "+layout["sourcePc2Sha256"]
        subprocess.run(["ffmpeg","-v","error","-n","-framerate",str(FPS),"-i",str(args.out/"%03d.jpg"),
            "-c:v","libx264","-threads","4","-preset","veryfast","-crf","17","-maxrate","9M","-bufsize","9M",
            "-profile:v","high","-level:v","5.2","-pix_fmt","yuv420p","-metadata","comment=FrankenTextiles "+source,
            "-movflags","+faststart",str(args.out/(name+".mp4"))],check=True)


if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("garment",choices=["sundress","eased","blue","yellow","bolero","trousers","column","flare","ensemble","fez"])
    p.add_argument("out",type=Path);p.add_argument("native",type=Path,nargs="?");p.add_argument("--proof",action="store_true")
    p.add_argument("--samples",type=int,default=5)
    p.add_argument("--engine",choices=["CYCLES","BLENDER_EEVEE_NEXT"],default="CYCLES")
    p.add_argument("--start-frame",type=int,default=0)
    p.add_argument("--end-frame",type=int,default=COUNT)
    args=p.parse_args(sys.argv[sys.argv.index("--")+1:])
    if not 0<=args.start_frame<args.end_frame<=COUNT:
        p.error(f"choose a nonempty frame interval within 0 through {COUNT}")
    args.out.mkdir(exist_ok=True,parents=True);render(args)
