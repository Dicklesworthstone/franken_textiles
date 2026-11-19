import * as T from "three";
import { applyNativeEyes, applyNativeSkin } from "../nativeSkin";

type Face = Record<"headTop"|"chin"|"eyeLeft"|"eyeRight",readonly number[]>;
const tau=2*Math.PI;
function sectionGrid(rows:number,columns:number,point:(u:number,v:number)=>number[],color?:(u:number,v:number)=>T.Color){
  const positions:number[]=[],colors:number[]=[],inds:number[]=[];
  for(let i=0;i<=rows;i++)for(let j=0;j<=columns;j++){
    const u=j/columns,v=i/rows;positions.push(...point(u,v));
    if(color){const c=color(u,v);colors.push(c.r,c.g,c.b);}
    const a=i*(columns+1)+j;if(i<rows&&j<columns)inds.push(a,a+1,a+columns+1,a+1,a+columns+2,a+columns+1);
  }
  const g=new T.BufferGeometry();g.setAttribute("position",new T.Float32BufferAttribute(positions,3));g.setIndex(inds);
  if(color)g.setAttribute("color",new T.Float32BufferAttribute(colors,3));
  g.computeVertexNormals();return g;
}

// Both atlas views were painted against the same orthographic capture of the
// reference mesh. Use rest-space centimetres so changing a tape never slides a feature.
function projectPortrait(material:T.MeshStandardMaterial,texture:T.Texture,restSpace:boolean,eye=false){
  const previous=material.onBeforeCompile, key=material.customProgramCacheKey();
  material.onBeforeCompile=function(shader,renderer){
    previous.call(this,shader,renderer);
    shader.uniforms.figurePortrait={value:texture};
    const pos=restSpace?"skinRest":"position";
    const attrs=eye?"attribute vec3 skinRest;\n":"";
    shader.vertexShader=attrs+"varying vec3 figurePoint; varying vec3 figureNormal;\n"+shader.vertexShader.replace(
      "#include <begin_vertex>",`#include <begin_vertex>\nfigurePoint=${pos}; figureNormal=normal;`);
    shader.fragmentShader="uniform sampler2D figurePortrait; varying vec3 figurePoint; varying vec3 figureNormal;\n"+
      shader.fragmentShader.replace("#include <alphamap_fragment>",`#include <alphamap_fragment>
      vec3 fp=figurePoint, fn=normalize(figureNormal);
      float y=(fp.y-155.5)*cos(.10);
      vec2 fv=vec2(.25+fp.x/52.25,138./304.+(y-fp.z*sin(.10))/26.125);
      vec2 sv=vec2(.75-fp.z/52.25,138./304.+(y-abs(fp.x)*sin(.10))/26.125);
      vec3 frontPigment=texture2D(figurePortrait,fv).rgb, sidePigment=texture2D(figurePortrait,sv).rgb;
      float edge=smoothstep(142.5,147.5,fp.y);
      float front=step(.002,fv.x)*step(fv.x,.498)*${eye?"1.0":"smoothstep(.04,.50,fn.z)*smoothstep(-3.,1.,fp.z)"};
      float side=step(.502,sv.x)*step(sv.x,.998)*smoothstep(.12,.68,abs(fn.x));
      ${eye?"":"front*=smoothstep(.9,1.12,frontPigment.r/max(frontPigment.g,.001));"}
      side*=smoothstep(.9,1.12,sidePigment.r/max(sidePigment.g,.001));
      diffuseColor.rgb=mix(diffuseColor.rgb,sidePigment,side*edge*.95);
      diffuseColor.rgb=mix(diffuseColor.rgb,frontPigment,front*edge*.97);
    `);
  };
  material.customProgramCacheKey=()=>key+(eye?"-fitted-iris":restSpace?"-fitted-face":"-coiffure");
}

export function styleFigure(g:T.BufferGeometry,f:Face,portrait:T.Texture|null){
  const p=g.getAttribute("position"),idx=g.getIndex()!,eyeY=f.eyeLeft[1],eyeZ=f.eyeLeft[2];
  const eyes={left:{value:new T.Vector3()},right:{value:new T.Vector3()}};
  const eye=new T.MeshStandardMaterial({color:"#ede0cf",roughness:.26});
  eye.onBeforeCompile=s=>{
    s.uniforms.eyeLeft=eyes.left;s.uniforms.eyeRight=eyes.right;
    s.vertexShader="varying vec3 eyePos;\n"+s.vertexShader.replace("#include <begin_vertex>","#include <begin_vertex>\neyePos=position;");
    s.fragmentShader="varying vec3 eyePos; uniform vec3 eyeLeft; uniform vec3 eyeRight;\n"+s.fragmentShader.replace("#include <color_fragment>",`#include <color_fragment>
      vec3 c = eyePos.x>0. ? eyeLeft : eyeRight;
      vec2 v = eyePos.xy-c.xy-vec2(-.035,.035); float r=length(v);
      if(eyePos.z>c.z) {
        float theta=atan(v.y,v.x);
        vec3 hazel=mix(vec3(.071,.049,.018),vec3(.22,.145,.052),.5+.5*sin(43.*theta+8.*r)*cos(19.*theta-12.*r));
        vec3 iris=mix(vec3(.027,.024,.013),hazel,1.-smoothstep(.40,.53,r));
        iris=mix(vec3(.009,.007,.005),iris,smoothstep(.21,.26,r));
        diffuseColor.rgb=mix(iris,diffuseColor.rgb,smoothstep(.51,.565,r));
        diffuseColor.rgb=mix(diffuseColor.rgb,vec3(.98,.92,.83),1.-smoothstep(.046,.075,length(v-vec2(-.17,.20))));
      }`);
  };
  applyNativeEyes(eye,"sundress");
  if(portrait)projectPortrait(eye,portrait,true,true);
  const skin=new T.MeshStandardMaterial({color:0xffffff,vertexColors:true,roughness:.7});
  applyNativeSkin(skin,"sundress");
  if(portrait)projectPortrait(skin,portrait,true);
  const skinColor=new T.Color("#c38a70"),lip=new T.Color("#a35c54"),blush=new T.Color("#c47c72");
  const colors=new Float32Array(p.count*3),color=new T.Color();
  for(let i=0;i<p.count;i++){
    const x=p.getX(i),y=p.getY(i)-eyeY,z=p.getZ(i)-eyeZ;
    color.copy(skinColor);
    if(z>-.9&&Math.abs(x)<7&&y>-10&&y<2){
      const mask=Math.exp(-(((Math.abs(x)-4.2)/2)**2)-(y+2.2)**2/2.9);
      color.lerp(blush,.56*mask);
      const center=-6.93+.10*(1-Math.cos(Math.min(Math.PI,Math.abs(x)*2.8)));
      const lipMask=(x/2.37)**2+((y-center)/(y>center?.63:.79))**2;
      color.lerp(lip,.9*(1-T.MathUtils.smoothstep(lipMask,.68,1.16)));
    }
    colors.set([color.r,color.g,color.b],i*3);
  }
  g.setAttribute("color",new T.BufferAttribute(colors,3));
  const head=new T.Group(),headTriangles:number[]=[];
  const skinEntries=g.groups[0]?.count??idx.count;
  for(let i=0;i<skinEntries;i+=3){
    const a=idx.getX(i),b=idx.getX(i+1),c=idx.getX(i+2);
    if(Math.min(p.getY(a),p.getY(b),p.getY(c))>f.chin[1]-.7)headTriangles.push(a,b,c);
  }
  const headG=new T.BufferGeometry();headG.setAttribute("position",new T.Float32BufferAttribute(p.array.slice(),3));headG.setIndex(headTriangles);
  const headMat=new T.MeshBasicMaterial({side:T.DoubleSide}),shell=new T.Mesh(headG,headMat);
  const picker=new T.Raycaster();
  function faceZ(x:number,y:number){
    picker.set(new T.Vector3(x,y,eyeZ+15),new T.Vector3(0,0,-1));return picker.intersectObject(shell,false)[0]?.point.z??eyeZ;
  }
  const hair=new T.MeshStandardMaterial({color:0xffffff,roughness:.62,vertexColors:true,side:T.DoubleSide});
  if(portrait)projectPortrait(hair,portrait,false);
  const dark=new T.Color("#352825"),chestnut=new T.Color("#664c40"),tip=new T.Color("#7b5949");
  const rootZ=f.headTop[2]-.55,topY=f.headTop[1];
  function sweep(u:number,v:number){
    const a=u*tau-Math.PI,front=Math.cos(a),s=Math.sin(a);
    const edge=eyeY-3.05+8.6*front-.48*Math.max(0,front)*Math.cos(a*2+.45)+.86*Math.sin(2*a+.18)*Math.max(0,front)+.12*Math.sin(5*a+.25);
    const scalpY=edge+(topY-.035-edge)*Math.sin(v*Math.PI/2);
    const dir=new T.Vector3(s,0,front),start=new T.Vector3(0,scalpY,rootZ).addScaledVector(dir,18);
    picker.set(start,dir.clone().negate());const hit=picker.intersectObject(shell,false)[0];
    const radius=hit?18-hit.distance:Math.sqrt(Math.max(0,1-((scalpY-(topY-10))/10)**2))*8;
    const flow=17*a+9*v+2.2*Math.sin(a+.5)*Math.sin(v*Math.PI);
    const wave=.06*Math.cos(flow)+.018*Math.cos(2*flow+.6);
    const lift=(.26+.99*Math.sin(Math.PI*v)**.65+wave)*Math.cos(v*Math.PI/2);
    return [s*(radius+lift),scalpY+.09+1.1*Math.sin(v*Math.PI/2)**1.5,rootZ+front*(radius+lift)];
  }
  function shade(u:number,v:number){
    const a=u*tau-Math.PI,stream=17*a+9*v+2.2*Math.sin(a+.5)*Math.sin(v*Math.PI);
    const base=dark.clone().lerp(chestnut,.40+.16*Math.cos(stream)+.08*Math.cos(2*stream+.6));
    return base.lerp(tip,.068*(.5+.5*Math.cos(9*stream)));
  }
  const cap=new T.Mesh(sectionGrid(27,180,sweep,shade),hair);cap.castShadow=true;head.add(cap);
  const capPoints=cap.geometry.getAttribute("position"),strands:number[]=[],strandColors:number[]=[];
  function capAt(a:number,t:number){
    const u=((a/tau+.5)%1+1)%1*180,v=T.MathUtils.clamp(t,0,1)*27;
    const col=Math.min(179,Math.floor(u)),row=Math.min(26,Math.floor(v)),index=row*181+col;
    const vec=(i:number)=>new T.Vector3().fromBufferAttribute(capPoints,i);
    const q=vec(index).lerp(vec(index+1),u-col).lerp(vec(index+181).lerp(vec(index+182),u-col),v-row);
    q.x+=.066*Math.sin(a);q.z+=.066*Math.cos(a);q.y+=.051;
    return q;
  }
  // Sweep fine strands out from an off-centre part. The colour variation follows each strand.
  for(let side=-1;side<=1;side+=2)for(let n=0;n<235;n++){
    const f=(n+.5)/235,partHeight=.12+.83*f,dest=side*(.12+2.89*f);
    const shade=dark.clone().lerp(tip,.18+.47*(.5+.5*Math.sin(n*241.7)));
    let prev:T.Vector3|undefined;
    for(let j=0;j<=40;j++){
      const t=j/40,a=-.28+dest*(.15*t+.85*Math.sin(t*Math.PI/2))+.07*Math.sin(t*Math.PI*2+f*7)*Math.sin(t*Math.PI);
      const h=partHeight*(1-t)+.16*Math.sin(Math.PI*t)*Math.sin(f*Math.PI)+.0035*Math.sin(n*3.8+8*t)*Math.sin(t*Math.PI);
      const point=capAt(a,h);
      if(prev){strands.push(...prev.toArray(),...point.toArray());strandColors.push(shade.r,shade.g,shade.b,shade.r,shade.g,shade.b);}
      prev=point;
    }
  }
  const locks=new T.BufferGeometry();locks.setAttribute("position",new T.Float32BufferAttribute(strands,3));
  locks.setAttribute("color",new T.Float32BufferAttribute(strandColors,3));
  head.add(new T.LineSegments(locks,new T.LineBasicMaterial({vertexColors:true,transparent:true,opacity:portrait ? .14 : .52})));
  // Braided low chignon, gathered into the native nape hairline.
  const bunG=sectionGrid(38,120,(u,v)=>{
    const a=tau*u,b=Math.PI*(v-.5),r=1+.043*Math.cos(9*a+5*b)+.025*Math.cos(18*a+10*b);
    return [5.4*r*Math.cos(b)*Math.sin(a),eyeY-.7+4.1*r*Math.sin(b),rootZ-7.25+4.25*r*Math.cos(b)*Math.cos(a)];
  },(u,v)=>shade(u+.24,v*1.6));
  const bun=new T.Mesh(bunG,hair);bun.castShadow=true;head.add(bun);
  for(const sign of [-1,1]){
    if(!portrait){
      const browG=sectionGrid(3,30,(u,v)=>{
        const x=sign*(1.37+3.55*u),y=eyeY+1.04+.56*Math.sin(Math.PI*u)-.09*u+
          (v-.5)*(.12+.34*Math.sin(Math.PI*(.16+.84*u)));
        return [x,y,faceZ(x,y)+.048];
      },(u,v)=>dark.clone().lerp(chestnut,.18+.45*v+.10*Math.sin(85*u)));
      head.add(new T.Mesh(browG,hair));
    }
    const pin=new T.Mesh(new T.SphereGeometry(.26,16,12),new T.MeshStandardMaterial({color:"#c6a46b",metalness:.63,roughness:.36}));
    pin.position.set(sign*7.83,eyeY-4.7,rootZ+.86);head.add(pin);
  }
  headG.dispose();headMat.dispose();
  const pivot=new T.Vector3((f.eyeLeft[0]+f.eyeRight[0])/2,eyeY,eyeZ);
  const restSpan=f.eyeLeft[0]-f.eyeRight[0],restHeight=f.headTop[1]-f.chin[1],restDepth=eyeZ-f.headTop[2];
  head.children.forEach(o=>o.position.sub(pivot));
  function apply(lm:Face){
    eyes.left.value.fromArray(lm.eyeLeft);eyes.right.value.fromArray(lm.eyeRight);
    head.position.set((lm.eyeLeft[0]+lm.eyeRight[0])/2,lm.eyeLeft[1],lm.eyeLeft[2]);
    head.scale.set((lm.eyeLeft[0]-lm.eyeRight[0])/restSpan,(lm.headTop[1]-lm.chin[1])/restHeight,(lm.eyeLeft[2]-lm.headTop[2])/restDepth);
  }
  apply(f);return {materials:[skin,eye],head,apply};
}
