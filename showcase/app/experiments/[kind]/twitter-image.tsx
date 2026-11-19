import { imageFile } from "../../../share/image";
import { experiments, type Rig } from "../../../components/materialExperiments";
type Props = {params: Promise<{kind:Rig}>};
export const generateStaticParams = () => Object.keys(experiments).map(kind=>({kind}));
export const size = {width:1200,height:600};
export const contentType = "image/png";
export const alt = "Rendered textile specimen from the native FrankenTextiles material experiment.";
export default async function Image({params}:Props) { return imageFile((await params).kind,600); }
