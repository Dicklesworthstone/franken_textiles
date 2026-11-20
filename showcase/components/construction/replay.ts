export type SewingPhases = {fold:number; zip:number; sew:number; drape:number};

// Keep the same chapter positions for every cut, even when settling takes hundreds of extra steps.
export function sewingFrame(frames:readonly number[], phases:SewingPhases, percent:number) {
  const wrapped=phases.fold-1, joined=wrapped+phases.zip, sewn=joined+phases.sew, end=frames[frames.length-1];
  const target=percent<45 ? wrapped*percent/44 : percent<=70 ? joined+(sewn-joined)*(percent-45)/25 : sewn+(end-sewn)*(percent-70)/30;
  return sampleIndex(frames,phases,target);
}

// The zip is a kinematic interpolation between two different starts. Hold the independent
// wrapped panels until the first admitted welded state, then cut directly to the solver.
export function displayFrame(frame:number, phases:Pick<SewingPhases,"fold"|"zip">, firstBodyFrame=phases.fold+phases.zip-1) {
  return frame < firstBodyFrame ? Math.min(frame,phases.fold-1) : frame;
}

export function sampleIndex(frames:readonly number[],phases:Pick<SewingPhases,"fold"|"zip">,requested:number,firstBodyFrame=phases.fold+phases.zip-1) {
  const target=displayFrame(requested,phases,firstBodyFrame), board=target<phases.fold;
  let best=-1;
  for(let i=0;i<frames.length;i++){
    const f=frames[i];
    if(board ? f>=phases.fold : f<firstBodyFrame)continue;
    if(best<0||Math.abs(f-target)<Math.abs(frames[best]-target))best=i;
  }
  if(best<0)throw Error("Garment replay is missing its cut or physical frames");
  return best;
}

// Each panel has its own interval on the horizontal axis; rows have disjoint vertical
// intervals. Recompute bounds after each bend, so even a sleeve turning sideways stays clear.
export function cuttingTable(points:Float32Array,ranges:readonly (readonly number[])[]) {
  const pieces=ranges.map(([a,b])=>({a,b,left:0,right:0,bottom:0,top:0,depth:0,dx:0,dy:0})).sort((a,b)=>a.a-b.a);
  const cols=Math.ceil(pieces.length/2);
  if(!cols)return ()=>{};
  let low=Infinity,high=-Infinity;
  for(let i=1;i<points.length;i+=3){low=Math.min(low,points[i]);high=Math.max(high,points[i]);}
  const center=(low+high)/2;
  return (xyz:Float32Array,gap=.035)=>{
    for(const p of pieces){
      p.left=p.bottom=Infinity;p.right=p.top=-Infinity;let near=Infinity,far=-Infinity;
      for(let i=p.a*3;i<p.b*3;i+=3){
        p.left=Math.min(p.left,xyz[i]);p.right=Math.max(p.right,xyz[i]);
        p.bottom=Math.min(p.bottom,xyz[i+1]);p.top=Math.max(p.top,xyz[i+1]);
        near=Math.min(near,xyz[i+2]);far=Math.max(far,xyz[i+2]);
      }
      p.depth=(near+far)/2;
    }
    let y=0;
    for(let start=0;start<pieces.length;start+=cols){
      const row=pieces.slice(start,start+cols);
      const width=row.reduce((sum,p)=>sum+p.right-p.left,0)+gap*(row.length-1);
      let x=-width/2;
      for(const p of row){p.dx=x-p.left;p.dy=y-p.top;x+=p.right-p.left+gap;}
      y-=Math.max(...row.map(p=>p.top-p.bottom))+gap;
    }
    for(const p of pieces)for(let i=p.a*3;i<p.b*3;i+=3){
      xyz[i]+=p.dx;xyz[i+1]+=p.dy+center-(y+gap)/2;xyz[i+2]-=p.depth;
    }
  };
}
