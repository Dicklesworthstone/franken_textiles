// These four standalone workbenches are copied from web/ and live outside the Next app router.
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { renderPlate, describePlate } from "../share/image";
import { plates, type PlateId } from "../share/catalog";

const origin=new URL(process.env.FTX_SITE_ORIGIN ?? "https://franken-textiles.com");
const tools: Record<string,PlateId>={index:"patternTool",photo:"photoTool",trace:"traceTool",materials:"materialTool"};
const out="public/workbench";
const escape=(value:string)=>value.replaceAll("&","&amp;").replaceAll('"',"&quot;").replaceAll("<","&lt;");
const meta=(attribute:"name"|"property",key:string,value:string)=>`<meta ${attribute}="${key}" content="${escape(value)}">`;
await mkdir(out+"/social",{recursive:true});
for (const [name,id] of Object.entries(tools)) {
  const card=plates[id];
  const title=card.title.join(" ").replace(/\.$/,"")+" · FrankenTextiles";
  const tags=[meta("name","description",card.caption),meta("property","og:type","website"),
    meta("property","og:title",title),meta("property","og:description",card.caption),
    meta("name","twitter:title",title),meta("name","twitter:description",card.caption),meta("name","twitter:card","summary_large_image")];
  for(const [network,height] of [["og",630],["twitter",600]] as const) {
    const {png,filename}=await renderPlate(id,height);
    await writeFile(out+"/social/"+filename,png);
    const attr=network==="og"?"property":"name";
    tags.push(meta(attr,network+":image",new URL("/workbench/social/"+filename,origin).href),meta(attr,network+":image:alt",describePlate(id)));
    if(network==="og") tags.push(meta(attr,"og:image:type","image/png"),meta(attr,"og:image:width","1200"),meta(attr,"og:image:height","630"));
  }
  const source=await readFile(`../web/${name}.html`,"utf8");
  await writeFile(`${out}/${name}.html`,source.replace("</head>",tags.join("\n")+"\n</head>"));
}
