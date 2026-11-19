import { notFound } from "next/navigation";
import MaterialLab from "../../../components/MaterialLab";
import { experiments } from "../../../components/materialExperiments";

export const dynamicParams = false;
export const generateStaticParams = () => Object.keys(experiments).map(kind => ({ kind }));
type Props = { params: Promise<{ kind: string }> };
async function rig({ params }: Props) {
  const { kind } = await params;
  if (kind !== "cylinder" && kind !== "strip") notFound();
  return kind;
}
export async function generateMetadata(props: Props) {
  const meta = experiments[await rig(props)];
  return { title: meta.title + " · FrankenTextiles", description: meta.subtitle };
}
export default async function ExperimentPage(props: Props) {
  return <MaterialLab kind={await rig(props)}/>;
}
