import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import sharp from "sharp";
import { plates, type Plate, type PlateId } from "./catalog";

type Artwork = { file: string; crop?: [left: number, top: number, width: number, height: number]; contain?: string };
const art: Record<string, Artwork> = {
  atelier: {file:"public/media/atelier-cover.webp"},
  dress: {file:"share/art/sundress.jpg"},
  hussar: {file:"share/art/hussar.jpg"},
  draft: {file:"share/art/draft.jpg",crop:[477,0,730,710]},
  cut: {file:"share/art/cut.jpg"},
  stitch: {file:"share/art/stitch.jpg"},
  strip: {file:"public/media/rig-strip.jpg",contain:"#afbeb6"},
  cylinder: {file:"public/media/rig-cylinder.jpg",crop:[260,0,780,720]},
  bolero: {file:"public/media/bolero.jpg",contain:"#212622"},
  construction: {file:"public/media/construction.jpg",contain:"#212622"},
  trace: {file:"public/labs/trace-overlay.webp"},
  source: {file:"public/labs/trace-bolero.png"},
  calibration: {file:"public/workbench/calibration/calib-sheet-a4.png"},
  "material-detail": {file:"public/media/rig-strip.jpg",crop:[426,70,445,590]},
};
const fontSpecs = [
  ["Instrument","instrument-serif","400","normal"],
  ["Instrument","instrument-serif","400","italic"],
  ["Sans","dm-sans","600","normal"],
  ["Mono","ibm-plex-mono","500","normal"],
] as const;
const bytes = (file: string) => readFile(join(process.cwd(), file));
const fonts = () => Promise.all(fontSpecs.map(async ([name,family,weight,style])=>({
  name, weight:Number(weight) as 400|500|600, style,
  data:await bytes(`node_modules/@fontsource/${family}/files/${family}-latin-${weight}-${style}.woff`),
})));

async function picture(key: string, width: number, height: number, whole=false) {
  const {file,crop,contain} = art[key];
  let img=sharp(await bytes(file));
  if(crop) {
    const [left,top,w,h]=crop;
    img=img.extract({left,top,width:w,height:h});
  }
  const data=await img.resize(width,height,{fit:whole&&contain?"contain":"cover",background:contain??"#202927"}).flatten({background:"#f0ede2"}).jpeg({quality:94}).toBuffer();
  return <img alt="" width={width} height={height} src={"data:image/jpeg;base64,"+data.toString("base64")}/>;
}

async function compose(id: PlateId,height: number) {
  const p:Plate=plates[id], [paper,ink,accent]=p.colors;
  const cover=p.layout==="cover", contact=p.layout==="contact", pair=p.layout==="pair";
  const film=id==="films", gutter=film?10:6;
  const artWidth=cover?1200:662, artHeight=cover?height:height-118;
  const largeHeight=Math.round((artHeight-gutter)*.58);
  const cellWidth=(artWidth-gutter)/2;
  const renderFonts=await fonts();
  const images=await Promise.all(p.images.map((image,i)=>{
    const width=pair?cellWidth:contact&&i>0?cellWidth:artWidth;
    const h=contact?(i===0?largeHeight:artHeight-largeHeight-gutter):artHeight;
    return picture(image,width,h,!pair&&!contact);
  }));
  const titleSize=p.title.some(t=>t.length>10)?88:108;
  return new ImageResponse(
    <div style={{display:"flex",position:"relative",width:1200,height,overflow:"hidden",background:paper,color:ink,fontFamily:"Sans"}}>
      {cover?<div style={{display:"flex",position:"absolute",inset:0}}>{images[0]}</div>:
        <div style={{display:"flex",position:"absolute",right:28,top:71,width:artWidth,height:artHeight,flexDirection:contact?"column":"row",gap:gutter}}>
          {images[0]}
          {pair&&images[1]}
          {contact&&<div style={{display:"flex",gap:gutter}}>{images[1]}{images[2]}</div>}
        </div>}
      {cover&&<div style={{display:"flex",position:"absolute",inset:0,background:"linear-gradient(90deg,rgba(12,24,29,.85),rgba(12,24,29,0.02) 90%)"}}/>}
      <div style={{display:"flex",position:"absolute",top:26,left:39,right:33,height:34,alignItems:"center",justifyContent:"space-between",color:cover?"#fff4dd":ink}}>
        <div style={{display:"flex",alignItems:"center",gap:10}}>
          <svg width="36" height="26" viewBox="0 0 36 26"><path fill={accent} d="M0 22L8 2h4L4 22zm8 3L17 1h6l-9 24zm10-2L26 3h4l-8 20zm8-3l6-15h4l-6 15z"/></svg>
          <span style={{display:"flex",fontSize:26,fontWeight:600,letterSpacing:-1.3}}>FrankenTextiles</span>
        </div>
        <span style={{display:"flex",fontFamily:"Mono",fontSize:15,letterSpacing:2,textTransform:"uppercase"}}>{p.category}</span>
      </div>
      <div style={{display:"flex",position:"absolute",left:41,top:72,right:32,height:1,background:cover?"#8a9287":ink,opacity:.4}}/>
      <div style={{display:"flex",position:"absolute",left:43,top:cover?150:146,width:cover?650:465,flexDirection:"column",alignItems:"flex-start"}}>
        <div style={{display:"flex",fontFamily:"Instrument",fontSize:cover?134:titleSize,lineHeight:1,letterSpacing:-3.8}}>{p.title[0]}</div>
        <div style={{display:"flex",fontFamily:"Instrument",fontStyle:"italic",fontSize:cover?142:titleSize+5,lineHeight:.92,letterSpacing:-3.8,color:accent}}>{p.title[1]}</div>
        <div style={{display:"flex",width:cover?490:420,marginTop:28,fontSize:23,lineHeight:1.32,fontWeight:600,letterSpacing:-.4}}>{p.caption}</div>
      </div>
      <div style={{display:"flex",position:"absolute",bottom:24,left:43,right:32,alignItems:"center",gap:12,fontFamily:"Mono",fontSize:14,letterSpacing:.6}}>
        <div style={{display:"flex",height:2,width:33,background:accent}}/>
        <span style={{display:"flex"}}>{p.specimen}</span>
      </div>
    </div>, {width:1200,height,fonts:renderFonts}
  );
}

// Including the rendered bytes in the public filename invalidates social caches when art, type or layout changes.
const cache = new Map<string,Promise<{png:Buffer; filename:string}>>();
export function renderPlate(id: PlateId,height: number) {
  const key = id + "-" + height;
  let result = cache.get(key);
  if (!result) {
    result = compose(id,height).then(async response=>{
      const png=Buffer.from(await response.arrayBuffer());
      if(png.length<24 || png.readUInt32BE(16)!==1200 || png.readUInt32BE(20)!==height) throw Error("Invalid social image for "+id);
      return {png,filename:createHash("sha256").update(png).digest("hex").slice(0,16)+".png"};
    });
    cache.set(key,result);
    result.catch(()=>cache.delete(key));
  }
  return result;
}
export function describePlate(id:PlateId) {
  const p=plates[id];
  return p.title.join(" ")+" "+p.specimen+".";
}
export async function imageMetadata(id:PlateId,height:number) {
  const {filename}=await renderPlate(id,height);
  return [{id:filename,alt:describePlate(id),size:{width:1200,height},contentType:"image/png"}];
}
export async function imageFile(id:PlateId,height:number) {
  const {png}=await renderPlate(id,height);
  return new Response(new Uint8Array(png),{headers:{"Content-Type":"image/png"}});
}
