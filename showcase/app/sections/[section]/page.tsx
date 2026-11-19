import { notFound } from "next/navigation";
import Exhibition from "../../../components/Exhibition";
import { isSection, plates } from "../../../share/catalog";

type Props = {params: Promise<{section:string}>};
async function sectionId({params}: Props) {
  const {section}=await params;
  if(!isSection(section)) notFound();
  return section;
}
export async function generateMetadata(props:Props) {
  const section=await sectionId(props), card=plates[section];
  return {title:card.title.join(" ").replace(/\.$/,"")+" · FrankenTextiles",description:card.caption,alternates:{canonical:`/sections/${section}/`}};
}
export default async function Section(props:Props) {
  return <Exhibition section={await sectionId(props)}/>;
}
