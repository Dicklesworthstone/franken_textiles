import { imageFile } from "../../../share/image";
import { sections, type Section } from "../../../share/catalog";
type Props = {params: Promise<{section:Section}>};
export const generateStaticParams = () => sections.map(section => ({section}));
export const size = {width:1200,height:630};
export const contentType = "image/png";
export const alt = "FrankenTextiles exhibition artwork featuring the tools or garments in this section.";
export default async function Image({params}:Props) { return imageFile((await params).section,630); }
