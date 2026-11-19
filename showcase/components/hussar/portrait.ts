import * as T from "three";
import {applyNativeEyes,applyNativeSkin} from "../nativeSkin";

type EyePosition = ArrayLike<number>;

export type FittedHussar = {
  head: T.BufferGeometry;
  leftHand: T.BufferGeometry;
  rightHand: T.BufferGeometry;
  eyeLeft: EyePosition;
  eyeRight: EyePosition;
  eyeRadius: number;
};
export type Complexion = { front:T.Texture; side:T.Texture };

// Coordinates in this shader stay in fitted-head centimetres before the exhibit pose.
const faceCoordinates = `
  varying vec3 portraitPosition;
  varying vec3 portraitNormal;
  uniform sampler2D frontalPigment;
  uniform sampler2D profilePigment;
  uniform vec3 eyeLeft;
  uniform vec3 eyeRight;
  float ovalFalloff(vec2 offset, vec2 radius) {
    vec2 distance = offset / radius;
    return exp(-dot(distance, distance) * 1.8);
  }
  float pigmentNoise(vec3 p) {
    return fract(sin(dot(floor(p), vec3(12.9898, 78.233, 32.711))) * 43758.5453);
  }
  vec3 paintedSkin(vec3 p, vec3 fallback) {
    vec3 n=normalize(portraitNormal);
    float heightMask=smoothstep(-.3,1.35,p.y)*(1.0-smoothstep(16.18,16.60,p.y));
    vec2 frontUV=vec2(.5+p.x/25.0,.5+(p.y-10.1)/25.0);
    vec2 sideUV=vec2(.5-(p.z+.2)/25.0,frontUV.y);
    float facing=smoothstep(.06,.66,n.z);
    float side=smoothstep(.14,.67,abs(n.x));
    float skin=heightMask*smoothstep(-10.6,-7.0,p.z);
    vec3 projected=mix(fallback,texture2D(profilePigment,sideUV).rgb,side*skin);
    return mix(projected,texture2D(frontalPigment,frontUV).rgb,facing*skin);
  }
`;

function paintSurface(
  material: T.MeshStandardMaterial,
  body: FittedHussar,
  complexion: Complexion,
  fragment: string,
  palette: Record<string, string>,
) {
  const uniforms = Object.fromEntries(Object.entries(palette).map(([name, color]) => [name, {value: new T.Color(color)}]));
  material.onBeforeCompile = shader => {
    Object.assign(shader.uniforms, uniforms, {
      eyeLeft: {value: new T.Vector3().fromArray(body.eyeLeft)},
      eyeRight: {value: new T.Vector3().fromArray(body.eyeRight)},
      frontalPigment: {value:complexion.front}, profilePigment:{value:complexion.side},
    });
    shader.vertexShader = "varying vec3 portraitPosition; varying vec3 portraitNormal;\n" + shader.vertexShader.replace(
      "#include <begin_vertex>", "#include <begin_vertex>\nportraitPosition = position; portraitNormal = normal;",
    );
    const colors = Object.keys(palette).map(key => "uniform vec3 " + key + ";").join("\n");
    shader.fragmentShader = colors + "\n" + faceCoordinates + shader.fragmentShader.replace(
      "#include <color_fragment>", "#include <color_fragment>\n" + fragment,
    );
  };
  material.customProgramCacheKey = () => fragment;
}

const skinPigment = `
  vec3 p = portraitPosition;
  vec3 eye = p.x > 0.0 ? eyeLeft : eyeRight;
  float lateral = abs(p.x) - abs(eye.x);
  float eyeHeight = eye.y;
  float front = smoothstep(eye.z - 3.5, eye.z - .5, p.z);
  float side = abs(p.x);
  float cheek = ovalFalloff(vec2(lateral - .8, p.y - eyeHeight + 2.8), vec2(2.1, 2.0));
  float nose = ovalFalloff(vec2(p.x, p.y - eyeHeight*.64), vec2(1.35, 1.3));
  float lipY = eyeHeight*.385 - .10*abs(p.x);
  float lips = ovalFalloff(vec2(p.x, p.y-lipY), vec2(2.32, .69));
  float orbit = ovalFalloff(vec2(lateral, p.y-eyeHeight), vec2(1.96, .94));
  float ear = smoothstep(6.7, 8.0, side) * ovalFalloff(vec2(p.z-eye.z+9.1, p.y-eyeHeight+2.0), vec2(3.2, 4.1));
  vec3 skin = mix(diffuseColor.rgb, blushColor, (.24*cheek+.16*nose)*front+.21*ear);
  skin = mix(skin, lipColor, lips*.97*front);
  skin *= 1.0-.19*orbit*front;
  float grain = pigmentNoise(p*33.0);
  skin *= .988 + grain*.024;

  // Pigment follows the fitted surface. The sculpt supplies the lips, eyelids, ears and nose.
  float browY = eyeHeight+1.04+.29*(1.0-lateral*lateral*.34)-lateral*.115;
  float browEnds = 1.0-smoothstep(1.05, 1.97, abs(lateral+.06));
  float brow = (1.0-smoothstep(.07,.33,abs(p.y-browY))) * browEnds * front;
  float fiber = .5+.5*sin(91.0*lateral-29.0*p.y+3.4*sin(lateral*13.0));
  skin = mix(skin, hairColor, brow*(.66+.15*fiber));

  float tip = clamp((side-.10)/2.25,0.0,1.0);
  float upperLip = eyeHeight*.456-.51*sin(tip*2.05)+.15*tip*tip;
  float moustacheWidth = .076+.26*(1.0-tip);
  float whiskers = (1.0-smoothstep(moustacheWidth*.52,moustacheWidth*1.45,abs(p.y-upperLip)))
    * smoothstep(.015,.24,side) * (1.0-smoothstep(.80,1.0,tip)) * front;
  skin = mix(skin, hairColor, whiskers*(.68+.16*grain));
  float beard = (1.0-smoothstep(eyeHeight*.29,eyeHeight*.58,p.y)) * smoothstep(.1,2.2,p.y);
  skin = mix(skin, hairColor, beard*front*(.025+.07*grain));

  float back = 1.0-smoothstep(eye.z-7.2, eye.z-1.0, p.z);
  float temple = smoothstep(4.0,7.2,side);
  float hairline = eyeHeight+5.7-3.2*temple-9.0*back;
  float scalp = smoothstep(-.09,.09,p.y-hairline) * (1.0-ear);
  float strand = .5+.5*sin(26.0*p.x+1.7*p.y+5.0*sin(p.z*.41+p.y*.23));
  skin = mix(skin, hairColor*(.84+.25*strand+.12*grain),scalp);
  diffuseColor.rgb = paintedSkin(p,skin);
`;

const eyePigment = `
  vec3 center = portraitPosition.x > 0.0 ? eyeLeft : eyeRight;
  vec3 relative = portraitPosition-center;
  vec2 gaze = relative.xy - vec2(.035,-.16);
  float r = length(gaze);
  float angle = atan(relative.y, relative.x);
  float spoke = .5+.5*sin(49.0*angle+sin(11.0*angle)*2.6+7.0*r);
  vec3 iris = mix(irisColor*.57, irisColor*1.4, spoke);
  iris *= .63+.37*(1.0-smoothstep(.47,.625,r));
  iris = mix(vec3(.009,.007,.005), iris, smoothstep(.255,.29,r));
  diffuseColor.rgb = mix(diffuseColor.rgb,iris,(1.0-smoothstep(.585,.64,r))*step(.05,relative.z));
  float catchlight = 1.0-smoothstep(.018,.054,distance(gaze,vec2(-.16,.14)));
  diffuseColor.rgb = mix(diffuseColor.rgb,vec3(.86,.81,.70),.66*catchlight*step(.05,relative.z));
  diffuseColor.rgb = mix(diffuseColor.rgb,paintedSkin(portraitPosition,diffuseColor.rgb), .83);
`;

export function buildWearer(body: FittedHussar, blue: boolean, maps: Complexion) {
  const complexion = blue ? "#bc876f" : "#aa785f";
  const skin = new T.MeshStandardMaterial({color: complexion, roughness:.63});
  const eyes = new T.MeshPhysicalMaterial({color:"#c3b5a3", roughness:.31, clearcoat:.38, clearcoatRoughness:.23});
  const hands = new T.MeshStandardMaterial({color:complexion, roughness:.69});
  const buckskin = new T.MeshStandardMaterial({color:"#bf9c68",roughness:.91});
  paintSurface(skin, body, maps, skinPigment, {
    blushColor:"#b87264", lipColor:"#954f51", hairColor:blue?"#483329":"#3d2e27",
  });
  paintSurface(eyes, body, maps, eyePigment, {irisColor:blue?"#485341":"#634a37"});
  applyNativeSkin(skin,"hussar");applyNativeSkin(hands,"hussar");applyNativeEyes(eyes,"hussar");
  const wearer = new T.Group();
  const handMeshes:T.Mesh[]=[];
  // Nest the cropped shoulder edge below the high stock. Visible facial geometry and eye groups stay in their native frame.
  const headGeometry = body.head.clone();
  const points = headGeometry.getAttribute("position");
  for(let i=0; i<points.count; i++){
    const x=points.getX(i), y=points.getY(i), z=points.getZ(i);
    if(y>=1.4)continue;
    const radius=Math.hypot(x/6.2,(z+1.1)/7.15);
    if(radius<=1)continue;
    const scale=T.MathUtils.lerp(1/radius,1,T.MathUtils.smoothstep(y,-.1,1.4));
    points.setXYZ(i,x*scale,y,(z+1.1)*scale-1.1);
  }
  headGeometry.computeVertexNormals();
  const head = new T.Mesh(headGeometry, [skin, eyes]);
  head.position.set(0, 151.4, .2);
  wearer.add(head);
  for (const [index, geometry] of [body.rightHand, body.leftHand].entries()) {
    const hand = new T.Mesh(geometry, hands);
    const sign = index === 0 ? -1 : 1;
    hand.position.set(sign*26.1, 86.5, 3.45);
    hand.rotation.y=-sign*.23;
    wearer.add(hand);handMeshes.push(hand);
  }
  return {wearer, gloves(on:boolean){for(const hand of handMeshes){hand.material=on?buckskin:hands;hand.scale.setScalar(on?1.025:1);}},
    dispose(){headGeometry.dispose();skin.dispose();eyes.dispose();hands.dispose();buckskin.dispose();}};
}
