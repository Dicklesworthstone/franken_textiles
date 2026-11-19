export default function TorsoSlice({body, cloth, otherCut = []}: {body:number[][]; cloth:number[][]; otherCut?:number[][]}) {
  const lines = [...body, ...cloth, ...otherCut];
  const xs = lines.flatMap(([x,,u]) => [x,u]), ys = lines.flatMap(([,y,,v]) => [y,v]);
  const left = Math.min(...xs)-8, top = Math.min(...ys)-30;
  const width = Math.max(...xs)-left+8, height = Math.max(...ys)-top+12;
  const path = (segments:number[][]) => segments.map(([a,b,c,d]) => `M${a} ${b}L${c} ${d}`).join("");
  return <svg viewBox={`${left} ${top} ${width} ${height}`} role="img" aria-label="Horizontal section of the torso and cloth, front of the wearer at the top">
    <text x={left+width/2} y={top+17} textAnchor="middle" fontSize="17" fill="#65574a">Front</text>
    <path d={path(body)} fill="none" stroke="#735042" strokeWidth="2.2"/><path d={path(cloth)} fill="none" stroke="#a72942" strokeWidth="2.2"/>
  </svg>;
}
