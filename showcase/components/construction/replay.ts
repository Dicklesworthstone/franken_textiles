export type SewingPhases = {fold:number; zip:number; sew:number; drape:number};

// Keep the same chapter positions for every cut, even when settling takes hundreds of extra steps.
export function sewingFrame(frames:readonly number[], phases:SewingPhases, percent:number) {
  const wrapped=phases.fold-1, sewn=wrapped+phases.zip+phases.sew, end=frames[frames.length-1];
  const target=percent<=45 ? wrapped*percent/45 : percent<=70 ? wrapped+(sewn-wrapped)*(percent-45)/25 : sewn+(end-sewn)*(percent-70)/30;
  let i=0;
  while(i+1<frames.length && Math.abs(frames[i+1]-target)<Math.abs(frames[i]-target))i++;
  return i;
}

// Translate whole cut pieces into two rows. Their shapes, grain and relative scale stay exact.
// Offsets disappear before seam closure, leaving the solver positions for sewing and gravity.
export function cuttingTable(points:Float32Array,ranges:readonly (readonly number[])[]) {
  const pieces=ranges.map(([a,b])=>{
    let left=Infinity,right=-Infinity,bottom=Infinity,top=-Infinity;
    for(let v=a;v<b;v++){left=Math.min(left,points[3*v]);right=Math.max(right,points[3*v]);
      bottom=Math.min(bottom,points[3*v+1]);top=Math.max(top,points[3*v+1]);}
    return {a,b,left,right,bottom,top,dx:0,dy:0};
  }).sort((a,b)=>a.a-b.a);
  const cols=Math.ceil(pieces.length/2),gap=.035;
  if(!cols)return ()=>{};
  let y=0;
  for(let start=0;start<pieces.length;start+=cols){
    const row=pieces.slice(start,start+cols);
    const width=row.reduce((sum,p)=>sum+p.right-p.left,0)+gap*(row.length-1);
    let x=-width/2;
    for(const p of row){p.dx=x-p.left;p.dy=y-p.top;x+=p.right-p.left+gap;}
    y-=Math.max(...row.map(p=>p.top-p.bottom))+gap;
  }
  const center=(Math.min(...pieces.map(p=>p.bottom))+Math.max(...pieces.map(p=>p.top)))/2-(y+gap)/2;
  return (xyz:Float32Array,unwrapped:number)=>{
    if(unwrapped<=0)return;
    const weight=unwrapped*unwrapped*(3-2*unwrapped);
    for(const p of pieces)for(let v=p.a;v<p.b;v++){xyz[3*v]+=weight*p.dx;xyz[3*v+1]+=weight*(p.dy+center);}
  };
}
