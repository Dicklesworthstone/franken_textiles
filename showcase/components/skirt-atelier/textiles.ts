import * as T from "three";

// Flat panel UVs use a 410 mm repeat and start at the hem. Binding and thread follow
// the moving cut, including through the sewing replay.
export function finishEdges(material:T.MeshPhysicalMaterial){
  material.onBeforeCompile=shader=>{
    shader.vertexShader="attribute float waistDistance; varying float fromWaist; varying vec2 panelMm;\n"+
      shader.vertexShader.replace("#include <uv_vertex>","#include <uv_vertex>\npanelMm=uv*410.0;fromWaist=waistDistance;");
    shader.fragmentShader="varying vec2 panelMm; varying float fromWaist;\n"+shader.fragmentShader.replace("#include <color_fragment>",`#include <color_fragment>
      float pixels=max(.2,fwidth(panelMm.y));
      float binding=1.-smoothstep(5.-pixels,5.+pixels,fromWaist);
      diffuseColor.rgb*=mix(vec3(1.),vec3(.54,.48,.48),binding);
      float distanceToStitch=min(abs(fromWaist-3.7),abs(panelMm.y-8.));
      float line=1.-smoothstep(.29,.29+pixels,distanceToStitch);
      float stitch=.5+.5*cos(panelMm.x*2.0944);
      diffuseColor.rgb*=1.+.38*line*stitch*max(0.,1.-fwidth(panelMm.x)/3.);
    `);
  };
  material.customProgramCacheKey=()=>"six-gore-bound-edges";
}

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
