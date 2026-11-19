import * as T from "three";
import {TessellateModifier} from "three/addons/modifiers/TessellateModifier.js";
import {soutache, type Point3} from "./haberdashery";
import type {Layer,Regiment} from "./RegimentalStudy";

export const sections={"Front":"front","Back":"back","Upper sleeve":"topsleeve","Cuff":"cuff","Collar":"collar"} as const;
export type Section=keyof typeof sections;
export type Mount="Curved" | "Flat";
type Point=[number,number];
type Edge={from:string;to:string;curve?:{type:string;c:Point[]}};
export type PatternPiece={id:string;name:string;note:string;cut:{qty:number;material:string};sa:number;
  vertices:Record<string,Point>;edges:Edge[];decor?:{kind:string;width_mm:number;pts:Point[]}[];
  internal?:{kind:string;pts:Point[]}[]};
export type Ensemble={pieces:PatternPiece[]};
export type PieceReading={id:string;widthMm:number;heightMm:number;allowance:number;cords:number};

export function selectedPiece(ensemble:Ensemble,layer:Exclude<Layer,"Dressed">,section:Section){
  const id=layer.toLowerCase()+"_"+sections[section],piece=ensemble.pieces.find(p=>p.id===id);
  if(!piece)throw Error("Missing cutting piece "+id);
  return piece;
}

/** All outlines, cord centre lines, button stations and dimensions use the downloadable pattern's
 * millimetres. Curved display wraps that same piece around a support without solving a cloth state. */
export function makeMount(piece:PatternPiece,regiment:Regiment,layout:Mount,woolMap:T.Texture){
  const group=new T.Group(), shape=new T.Shape(), edge=piece.edges[0];
  shape.moveTo(...piece.vertices[edge.from]);
  for(const {from,to,curve} of piece.edges){
    const [x,y]=piece.vertices[from], [ex,ey]=piece.vertices[to],dx=ex-x,dy=ey-y;
    if(!curve){shape.lineTo(ex,ey);continue;}
    const absolute=([t,h]:Point):Point=>[x+t*dx-h*dy,y+t*dy+h*dx];
    if(curve.type==="cubic")shape.bezierCurveTo(...absolute(curve.c[0]),...absolute(curve.c[1]),ex,ey);
    else if(curve.type==="quadratic")shape.quadraticCurveTo(...absolute(curve.c[0]),ex,ey);
    else throw Error("Unsupported edge on "+piece.id);
  }
  shape.closePath();
  const bounds=new T.Box2().setFromPoints(shape.getPoints(24)),size=bounds.getSize(new T.Vector2()),centre=bounds.getCenter(new T.Vector2());
  const face=piece.id.endsWith("_front"), paired=face&&layout==="Curved",outer=piece.id.startsWith("pelisse");
  const dyed=outer&&regiment==="Yellow"?"#ba914d":regiment==="Blue"?"#25364d":"#222826";
  const material=new T.MeshPhysicalMaterial({color:dyed,roughness:.92,sheen:.44,sheenRoughness:.87,sheenColor:"#888783",
    side:T.DoubleSide,map:woolMap,bumpMap:woolMap,bumpScale:.028});
  const cord=new T.MeshStandardMaterial({color:regiment==="Blue"?"#c5a05b":"#c4c0b2",roughness:.82,metalness:.04,side:T.DoubleSide});
  const metal=new T.MeshStandardMaterial({color:regiment==="Blue"?"#a58545":"#ada797",roughness:.32,metalness:.80});
  const hem=new T.MeshStandardMaterial({color:dyed,roughness:.79});
  const lining=new T.MeshStandardMaterial({color:outer?"#9d9583":"#99968a",roughness:.98,side:T.DoubleSide});
  const top=125+size.y/20;
  function locate(x:number,y:number,sign:number,lift=0):Point3{
    const sy=top+(y-bounds.max.y)/10;
    if(paired){
      const chest=T.MathUtils.smoothstep(y,bounds.min.y,bounds.max.y*.80);
      const radius=14.4+2.4*chest,angle=(Math.abs(x)/10+.13)/radius;
      return [sign*(radius+lift)*Math.sin(angle),sy,(radius*.84+lift)*Math.cos(angle)-4];
    }
    const local=(x-centre.x)/10;
    if(layout==="Flat")return [local,sy,lift];
    const radius=Math.max(10,size.x/19),angle=local/radius;
    return [(radius+lift)*Math.sin(angle),sy,(radius+lift)*Math.cos(angle)-radius];
  }
  const add=(geo:T.BufferGeometry,mat:T.Material)=>{const m=new T.Mesh(geo,mat);m.castShadow=m.receiveShadow=true;group.add(m);return m;};
  for(const sign of paired?[-1,1]:[1]){
    const raw=new T.ShapeGeometry(shape,20);
    const refined=new TessellateModifier(16,8).modify(raw);raw.dispose();
    for(const [offset,mat] of [[0,material],[-.12,lining]] as const){
      const g=refined.clone(),attr=g.getAttribute("position"),uv=g.getAttribute("uv");
      for(let i=0;i<attr.count;i++){const x=attr.getX(i),y=attr.getY(i);uv.setXY(i,x/93,y/93);attr.setXYZ(i,...locate(x,y,sign,offset));}
      g.computeVertexNormals();add(g,mat);
    }
    refined.dispose();
    const perimeter=shape.getSpacedPoints(170).map(p=>new T.Vector3(...locate(p.x,p.y,sign,-.05)));
    add(new T.TubeGeometry(new T.CatmullRomCurve3(perimeter,true),180,.085,6,true),hem);
    for(const decor of piece.decor??[]){
      if(decor.kind!=="soutache"||decor.pts.length<2)continue;
      const samples=decor.pts.map(([x,y])=>locate(x,y,sign,decor.width_mm*.023));
      add(soutache(samples,decor.width_mm*.05),cord);
    }
    for(const mark of piece.internal??[]){
      if(mark.kind!=="draw"||mark.pts.length===0||!face)continue;
      const [x,y]=mark.pts[0],at=locate(x,y,sign,.15),above=locate(x,y,sign,.5);
      const axis=new T.Vector3(...above).sub(new T.Vector3(...at)).normalize();
      const button=add(new T.SphereGeometry(.42,20,14),metal);
      button.position.set(...at);button.quaternion.setFromUnitVectors(new T.Vector3(0,0,1),axis);button.scale.z=.64;
    }
  }
  const extent=new T.Box3().setFromObject(group).getSize(new T.Vector3());
  const reading:PieceReading={id:piece.id,widthMm:size.x,heightMm:size.y,allowance:piece.sa,
    cords:(piece.decor??[]).filter(d=>d.kind==="soutache").length};
  return {group,extent,piece,reading,dispose(){group.traverse(o=>{if(o instanceof T.Mesh)o.geometry.dispose();});[material,cord,metal,hem,lining].forEach(m=>m.dispose());}};
}
