import type { MeshStandardMaterial } from "three";

// Original procedural skin detail on the synthetic adult fitted surfaces. Region channels were
// sampled at the source MakeHuman OBJ's corner UVs from the pinned Anny wheel's CC0 MPFB masks.
// Face, lips, eyelids, ears / fingernails, toenails, mouth interior, scattering response.
const varyings = `
  varying vec4 surfaceZones;
  varying vec4 surfaceDetails;
  varying vec3 pigmentPosition;
`;
const noiseFunctions = `
  float pigmentHash(vec3 p) {
    p = fract(p * vec3(.1031,.1030,.0973));
    p += dot(p,p.yzx+33.33);
    return fract((p.x+p.y)*p.z);
  }
  float pigmentNoise(vec3 p) {
    vec3 cell=floor(p), w=fract(p);
    w=w*w*(3.-2.*w);
    return mix(mix(mix(pigmentHash(cell),pigmentHash(cell+vec3(1,0,0)),w.x),
                   mix(pigmentHash(cell+vec3(0,1,0)),pigmentHash(cell+vec3(1,1,0)),w.x),w.y),
               mix(mix(pigmentHash(cell+vec3(0,0,1)),pigmentHash(cell+vec3(1,0,1)),w.x),
                   mix(pigmentHash(cell+vec3(0,1,1)),pigmentHash(cell+vec3(1,1,1)),w.x),w.y),w.z);
  }
`;

export function applyNativeSkin(material:MeshStandardMaterial, garment:"hussar"|"sundress") {
  const inherited=material.onBeforeCompile;
  const originalKey=material.customProgramCacheKey();
  material.onBeforeCompile=function(shader,renderer) {
    inherited.call(this,shader,renderer);
    shader.vertexShader="attribute vec4 skinZones; attribute vec4 skinDetail; attribute vec3 skinRest;\n"+
      varyings+shader.vertexShader.replace("#include <begin_vertex>",`#include <begin_vertex>
        surfaceZones=skinZones; surfaceDetails=skinDetail; pigmentPosition=skinRest;`);
    shader.fragmentShader=varyings+noiseFunctions+shader.fragmentShader
      .replace("#include <alphamap_fragment>",`
        float faceZone=surfaceZones.r;
        float lipZone=surfaceZones.g;
        float lidZone=surfaceZones.b;
        float earZone=surfaceZones.a;
        float nailZone=max(surfaceDetails.r,surfaceDetails.g);
        float mouthZone=surfaceDetails.b;
        vec3 cm=pigmentPosition;
        float visibleGrain=1.-smoothstep(.012,.12,max(length(dFdx(cm)),length(dFdy(cm))));
        float cloud=pigmentNoise(cm*.63+13.);
        float blush=pigmentNoise(cm*.23+4.)-.5;
        float ridge=pigmentNoise(cm*vec3(18.,28.,21.)+27.);
        float pore=smoothstep(.53,.86,pigmentNoise(cm*34.+18.))*visibleGrain;
        float pigmentContrast=(cloud-.5)*.064+(ridge-.5)*visibleGrain*.034;
        vec2 skinCheekOffset=vec2((abs(cm.x)-3.7)/3.0,(cm.y-${garment==="hussar"?"10.8":"153.8"})/3.8);
        float cheek=exp(-dot(skinCheekOffset,skinCheekOffset));
        float fleck=smoothstep(.76,.90,pigmentNoise(cm*3.9+54.))*(.12+faceZone*(.24+.64*cheek));
        // Epidermal melanin variation and clustered freckles are pigment, separate from
        // the capillary tint around thin skin and the wet regions of the face.
        diffuseColor.rgb*=vec3(1.+blush*.10,1.-blush*.043,1.-blush*.063)*(1.+pigmentContrast);
        diffuseColor.rgb*=mix(vec3(1.),vec3(.64,.51,.40),fleck*.20);
        diffuseColor.rgb*=mix(vec3(1.),vec3(1.12,.88,.84),earZone*.63+lidZone*.20);
        diffuseColor.rgb*=mix(vec3(1.),vec3(1.18,.80,.85),lipZone*.78);
        diffuseColor.rgb*=mix(vec3(1.),vec3(.47,.24,.27),mouthZone*.86);
        diffuseColor.rgb=mix(diffuseColor.rgb,diffuseColor.rgb*vec3(1.09,1.01,.98)+.022,nailZone*.88);
        #include <alphamap_fragment>`)
      .replace("#include <roughnessmap_fragment>",`#include <roughnessmap_fragment>
        roughnessFactor=clamp(roughnessFactor+(cloud-.5)*.095-faceZone*.065-lipZone*.19-lidZone*.13-nailZone*.21-pore*.05,.22,.86);`)
      .replace("#include <normal_fragment_maps>",`#include <normal_fragment_maps>
        // Perturb the shading normal from the continuous rest-space height field. Filtering
        // with the screen footprint avoids sparkling while the visitor orbits or zooms.
        vec3 alongX=dFdx(-vViewPosition),alongY=dFdy(-vViewPosition);
        vec3 acrossX=cross(alongY,normal),acrossY=cross(normal,alongX);
        float normDet=dot(alongX,acrossX);
        float relief=((ridge-.5)*.0017-pore*.003)*(1.-nailZone*.85);
        normal=normalize(abs(normDet)*normal-sign(normDet)*(dFdx(relief)*acrossX+dFdy(relief)*acrossY));`)
      .replace("#include <lights_fragment_end>",`#include <lights_fragment_end>
        float grazing=pow(1.-max(0.,dot(geometryViewDir,normal)),2.);
        float thinSkin=earZone*.19+lidZone*.10+lipZone*.075+.028*surfaceDetails.a;
        reflectedLight.indirectDiffuse+=diffuseColor.rgb*vec3(.76,.18,.09)*grazing*thinSkin;`);
  };
  material.customProgramCacheKey=()=>originalKey+"-mpfb-surface-"+garment;
  material.needsUpdate=true;
}

/** Finish the two native eye surfaces using the fitted eyeball centers supplied by each exhibit. */
export function applyNativeEyes(material:MeshStandardMaterial, garment:"hussar"|"sundress") {
  const inherited=material.onBeforeCompile;
  const originalKey=material.customProgramCacheKey();
  material.onBeforeCompile=function(shader,renderer) {
    inherited.call(this,shader,renderer);
    shader.vertexShader="varying vec3 ocularPosition;\n"+shader.vertexShader.replace(
      "#include <begin_vertex>","#include <begin_vertex>\nocularPosition=position;");
    shader.fragmentShader="varying vec3 ocularPosition;\n"+noiseFunctions+shader.fragmentShader
      .replace("#include <alphamap_fragment>",`
        vec3 eyeRelative=ocularPosition-(ocularPosition.x>0.?eyeLeft:eyeRight);
        vec2 gazeXY=eyeRelative.xy-${garment==="hussar"?"vec2(.035,-.16)":"vec2(-.035,.035)"};
        float orbitRadius=length(gazeXY);
        float angleO=atan(gazeXY.y,gazeXY.x);
        float edgeT=smoothstep(.65,1.27,orbitRadius);
        float cornealFace=smoothstep(.2,.75,eyeRelative.z);
        float rayT=.5+.5*sin(angleO*82.+2.7*sin(angleO*29.)+9.*orbitRadius);
        float irisBand=smoothstep(.25,.31,orbitRadius)*(1.-smoothstep(.48,.57,orbitRadius));
        diffuseColor.rgb*=1.+(rayT-.5)*.26*irisBand*cornealFace;
        float collar=smoothstep(.29,.34,orbitRadius)*(1.-smoothstep(.35,.42,orbitRadius));
        diffuseColor.rgb*=mix(vec3(1.),vec3(1.22,1.08,.87),collar*rayT*cornealFace);
        // Fine arcing conjunctival vessels originate near the corners, fading before the limbus.
        float vesselPath=sin(8.*angleO+1.3*sin(angleO*12.)+orbitRadius*(4.+sin(angleO*7.)));
        float vessel=1.-smoothstep(.025,.12+fwidth(vesselPath),abs(vesselPath));
        diffuseColor.rgb*=mix(vec3(1.),vec3(1.04,.79,.77),edgeT*cornealFace*(.31+.57*vessel));
        #include <alphamap_fragment>`)
      .replace("#include <roughnessmap_fragment>",`#include <roughnessmap_fragment>
        roughnessFactor=mix(roughnessFactor,.18,cornealFace*(1.-smoothstep(.53,.66,orbitRadius))*.78);`);
  };
  material.customProgramCacheKey=()=>originalKey+"-corneal-surface-"+garment;
  material.needsUpdate=true;
}
