import * as T from "three";

// Upholstery weave for the optional tailoring form.
export function linenCover() {
  const tile=document.createElement("canvas");tile.width=tile.height=256;
  const ctx=tile.getContext("2d")!;
  ctx.fillStyle="#d7d0bc";ctx.fillRect(0,0,256,256);
  for(let i=0;i<256;i+=2){
    ctx.fillStyle=i%6===0?"rgba(255,236,211,.12)":"rgba(25,22,17,.09)";
    ctx.fillRect(0,i,256,1);ctx.fillRect(i,0,1,256);
  }
  const map=new T.CanvasTexture(tile);map.colorSpace=T.SRGBColorSpace;
  map.wrapS=map.wrapT=T.RepeatWrapping;map.anisotropy=8;
  return map;
}
