import { notFound } from "next/navigation";
import MaterialLab from "../../../components/MaterialLab";
import { experiments } from "../../../components/materialExperiments";

type Props = { params: Promise<{ kind: string }> };
async function rig({ params }: Props) {
  const { kind } = await params;
  if (kind !== "cylinder" && kind !== "strip") notFound();
  return kind;
}
export async function generateMetadata(props: Props) {
  const kind = await rig(props), meta = experiments[kind];
  return { title: meta.title + " · FrankenTextiles", description: meta.subtitle, alternates: { canonical: `/experiments/${kind}/` } };
}
export default async function ExperimentPage(props: Props) {
  return <MaterialLab kind={await rig(props)}/>;
}
