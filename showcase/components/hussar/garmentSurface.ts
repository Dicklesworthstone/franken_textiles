import * as T from "three";

export type GarmentMaps = {
  wool:T.Texture;
  pelisses:T.Texture;
  Blue:{ Dressed:T.Texture; Dolman:T.Texture };
  Yellow:{ Dressed:T.Texture; Dolman:T.Texture };
};

export async function loadGarmentMaps(loader:T.TextureLoader):Promise<GarmentMaps> {
  async function sample(name:string) {
    const map=await loader.loadAsync("/wardrobe/hussar/"+name+".webp");
    map.colorSpace=T.SRGBColorSpace;map.anisotropy=4;
    return map;
  }
  const [wool,pelisses,bd,bi,yd,yi]=await Promise.all(
    ["broadcloth","pelisses","blue-dressed","blue-dolman","yellow-dressed","yellow-dolman"].map(sample));
  wool.wrapS=wool.wrapT=T.RepeatWrapping;wool.repeat.set(12,6);
  return {wool,pelisses,Blue:{Dressed:bd,Dolman:bi},Yellow:{Dressed:yd,Dolman:yi}};
}

export function disposeGarmentMaps(maps:GarmentMaps) {
  [maps.wool,maps.pelisses,maps.Blue.Dressed,maps.Blue.Dolman,maps.Yellow.Dressed,maps.Yellow.Dolman].forEach(t=>t.dispose());
}

/** Register a garment to its synthetic albedo study in exhibit centimetres; preserve geometry, layers and rear surface materials. */
export function registerGarment(group:T.Object3D,coatOffset?:number) {
  group.updateMatrixWorld(true);
  const normalMatrix=new T.Matrix3(), p=new T.Vector3(), n=new T.Vector3();
  group.traverse(mesh=>{
    if(!(mesh instanceof T.Mesh))return;
    const geo=mesh.geometry as T.BufferGeometry, points=geo.getAttribute("position"), normals=geo.getAttribute("normal");
    if(!normals)return;
    const data=new Float32Array(points.count*3);
    normalMatrix.getNormalMatrix(mesh.matrixWorld);
    for(let i=0;i<points.count;i++){
      p.fromBufferAttribute(points,i).applyMatrix4(mesh.matrixWorld);
      n.fromBufferAttribute(normals,i).applyNormalMatrix(normalMatrix);
      data[i*3]=coatOffset===undefined ? .5+p.x/(212*2/3) : .5+(p.x+coatOffset)/153.6;
      data[i*3+1]=coatOffset===undefined ? .5+(p.y-102.5)/212 : .5+(p.y-117)/96;
      data[i*3+2]=T.MathUtils.smoothstep(n.z,-.02,.60);
    }
    geo.setAttribute("wardrobeStudy",new T.BufferAttribute(data,3));
  });
}

export function applyGarmentStudy(material:T.MeshStandardMaterial, selected:{value:T.Texture}, amount:number) {
  material.onBeforeCompile=shader=>{
    shader.uniforms.garmentStudy=selected;
    shader.uniforms.garmentDye={value:material.color};
    shader.uniforms.garmentAmount={value:amount};
    shader.vertexShader="attribute vec3 wardrobeStudy; varying vec3 clothRegistration;\n"+shader.vertexShader.replace(
      "#include <begin_vertex>","#include <begin_vertex>\nclothRegistration=wardrobeStudy;");
    shader.fragmentShader="uniform sampler2D garmentStudy; uniform vec3 garmentDye; uniform float garmentAmount; varying vec3 clothRegistration;\n"+shader.fragmentShader.replace(
      "#include <color_fragment>",`#include <color_fragment>
      vec3 fabricSample=texture2D(garmentStudy,clothRegistration.xy).rgb;
      float sampleL=dot(fabricSample,vec3(.2126,.7152,.0722));
      float dyeL=dot(garmentDye,vec3(.2126,.7152,.0722));
      float chromaDistance=length(normalize(fabricSample+vec3(.002))-normalize(garmentDye+vec3(.002)));
      // Reject samples falling outside this textile, e.g. pale crossbelt pixels on the indigo jacket.
      float matchHue=1.-smoothstep(.40,.83,chromaDistance);
      float matchLum=smoothstep(.035,.17,(sampleL+.001)/(dyeL+.001))*
        (1.-smoothstep(3.8,8.0,(sampleL+.001)/(dyeL+.001)));
      float finish=clothRegistration.z * garmentAmount * matchHue * matchLum;
      diffuseColor.rgb=mix(diffuseColor.rgb,fabricSample*.91,finish);
    `);
  };
  material.customProgramCacheKey=()=>"hussar-surface-plate-v1";
}
