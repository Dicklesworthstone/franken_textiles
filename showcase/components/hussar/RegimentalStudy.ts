import * as THREE from "three";

export type Regiment = "Blue" | "Yellow";
export type RegimentView = Regiment | "Compare";
export type Layer = "Dressed" | "Dolman" | "Pelisse";
export type Framing = "Uniform" | "Bust" | "Chest" | "Side" | "Reverse";

/** Eight registered costume illustrations share one reconstructed 3D silhouette volume. */
export async function readStudy(regiment:Regiment) {
  const base="/wardrobe/hussar/"+regiment.toLowerCase();
  const loader=new THREE.TextureLoader();
  const [response,atlas,flow]=await Promise.all([fetch(base+"-hull.gz"),loader.loadAsync(base+"-views.webp"),loader.loadAsync(base+"-flow.webp")]);
  if(!response.ok)throw Error("Could not load the "+regiment+" costume surface");
  const file=await response.arrayBuffer();
  const bytes=new Uint8Array(file);
  const payload=bytes[0]===31&&bytes[1]===139
    ? await new Response(new Blob([file]).stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()
    : file;
  const [vertexCount,faceCount]=new Uint32Array(payload,0,2);
  if(payload.byteLength!==8+6*vertexCount+6*faceCount)throw Error("Invalid costume surface");
  const points=new Int16Array(payload,8,vertexCount*3);
  const geometry=new THREE.BufferGeometry();
  geometry.setAttribute("position",new THREE.BufferAttribute(Float32Array.from(points,v=>v*.01),3));
  geometry.setIndex(new THREE.BufferAttribute(new Uint16Array(payload,8+points.byteLength,3*faceCount),1));
  geometry.computeVertexNormals();geometry.computeBoundingSphere();
  atlas.colorSpace=THREE.SRGBColorSpace;atlas.anisotropy=4;
  const direction={value:0};
  flow.generateMipmaps=false;flow.minFilter=THREE.LinearFilter;
  const material=new THREE.ShaderMaterial({side:THREE.DoubleSide,uniforms:{elevations:{value:atlas},offsets:{value:flow},azimuth:direction},
    vertexShader:`varying vec3 local;
      void main(){local=position;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}`,
    fragmentShader:`varying vec3 local;uniform sampler2D elevations;uniform sampler2D offsets;uniform float azimuth;
      vec2 sampleUV(float id){
        float a=id*0.78539816339;
        float horizontal=local.x*cos(a)-local.z*sin(a);
        vec2 tile=clamp(vec2(.5+4.5*horizontal/420.,(14.+4.5*local.y)/960.),.002,.998);
        return (tile+vec2(mod(id,4.),1.-floor(id/4.)))/vec2(4.,2.);
      }
      void main(){
        float heading=mod(azimuth/0.78539816339+8.,8.);
        float index=floor(heading);
        vec2 pa=sampleUV(index),pb=sampleUV(mod(index+1.,8.));
        float f=fract(heading);
        vec2 scale=vec2(1./(1680.*3.),-1./(1920.*3.));
        vec2 da=(texture2D(offsets,pa).ba*255.-128.)*scale;
        vec2 db=(texture2D(offsets,pb).rg*255.-128.)*scale;
        vec4 a=texture2D(elevations,pa+da*f),b=texture2D(elevations,pb+db*(1.-f));
        float t=smoothstep(.39,.61,f);
        if(mix(a.a,b.a,t)<.29)discard;
        vec2 weights=vec2(1.-t,t)*max(vec2(.04),vec2(a.a,b.a));
        gl_FragColor=vec4((a.rgb*weights.x+b.rgb*weights.y)/(weights.x+weights.y),1.);
        #include <colorspace_fragment>
      }`});
  const figure=new THREE.Mesh(geometry,material);
  figure.castShadow=true;
  return {figure,angle:(yaw:number)=>{direction.value=yaw;},
    dispose(){geometry.dispose();material.dispose();atlas.dispose();flow.dispose();}};
}
