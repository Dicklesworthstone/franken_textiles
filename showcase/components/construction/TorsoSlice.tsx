export default function TorsoSlice({body, cloth, otherCut = [], compare = false}: {body:number[][]; cloth:number[][]; otherCut?:number[][]; compare?:boolean}) {
  const lines = [...body, ...cloth, ...otherCut];
  const xs = lines.flatMap(([x,,u]) => [x,u]), ys = lines.flatMap(([,y,,v]) => [y,v]);
  const left = Math.min(...xs)-8, top = Math.min(...ys)-30;
  const width = Math.max(...xs)-left+8, height = Math.max(...ys)-top+12;
  const path = (segments:number[][]) => segments.map(([a,b,c,d]) => `M${a} ${b}L${c} ${d}`).join("");
  const torsoX = body.flatMap(([x,,u]) => [x,u]);
  const centre = (Math.min(...torsoX) + Math.max(...torsoX)) / 2;
  return <svg viewBox={`${left} ${top} ${width} ${height}`} role="img" aria-label={compare ? "Both waist cuts on the same torso. A dotted centreline crosses the front and back measurement positions." : "Horizontal section of the torso and cloth, front of the wearer at the top"}>
    <text x={left+width/2} y={top+17} textAnchor="middle" fontSize="17" fill="#65574a">Front</text>
    <path d={path(body)} fill="none" stroke="#735042" strokeWidth="2.2"/><path d={path(cloth)} fill="none" stroke="#a72942" strokeWidth="2.2"/>
    {compare && <><path d={path(otherCut)} fill="none" stroke="#086d69" strokeWidth="2.2"/>
      <line x1={centre} y1={top+23} x2={centre} y2={top+height-3} stroke="#5d655b" strokeWidth="1.5" strokeDasharray="3 4"/></>}
  </svg>;
}
