type Ruler = {x:number; bodyY:number; clothY:number; description:string};
export default function TorsoSlice({body, cloth, otherCut = [], compare = false, ruler}: {body:number[][]; cloth:number[][]; otherCut?:number[][]; compare?:boolean; ruler?:Ruler}) {
  const lines = [...body, ...cloth, ...otherCut];
  const xs = lines.flatMap(([x,,u]) => [x,u]), ys = lines.flatMap(([,y,,v]) => [y,v]);
  const left = Math.min(...xs)-8, top = Math.min(...ys)-30;
  const width = Math.max(...xs)-left+8, height = Math.max(...ys)-top+12;
  const path = (segments:number[][]) => segments.map(([a,b,c,d]) => `M${a} ${b}L${c} ${d}`).join("");
  const torsoX = body.flatMap(([x,,u]) => [x,u]);
  const centre = ruler?.x ?? (Math.min(...torsoX) + Math.max(...torsoX)) / 2;
  return <svg viewBox={`${left} ${top} ${width} ${height}`} role="img" aria-label={compare ? "Both waist cuts on the same torso. "+(ruler?.description ?? "The dotted line marks the centreline.") : "Horizontal section of the torso and cloth, front of the wearer at the top"}>
    <text x={left+width/2} y={top+17} textAnchor="middle" fontSize="17" fill="#65574a">Front</text>
    <path d={path(body)} fill="none" stroke="#735042" strokeWidth="2.2"/><path d={path(cloth)} fill="none" stroke="#a72942" strokeWidth="2.2"/>
    {compare && <><path d={path(otherCut)} fill="none" stroke="#086d69" strokeWidth="2.2"/>
      <line x1={centre} y1={top+23} x2={centre} y2={top+height-3} stroke="#5d655b" strokeWidth="1.5" strokeDasharray="3 4"/></>}
    {ruler && <g stroke="#182d37" strokeWidth="2.5">
      <line data-ruler="mm" x1={ruler.x} x2={ruler.x} y1={ruler.bodyY} y2={ruler.clothY}/>
      {[ruler.bodyY,ruler.clothY].map((y,i)=><line key={i} x1={ruler.x-6} y1={y} x2={ruler.x+6} y2={y}/>)}
    </g>}
  </svg>;
}
