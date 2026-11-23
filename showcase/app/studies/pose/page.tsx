import type { Metadata } from "next";
import PoseRoom from "../../../components/doll/PoseRoom";

export const metadata: Metadata = {
  title: "Pose the figure · FrankenTextiles",
  description:
    "Drag a form's arms and watch a bolero follow: the project's own cloth solver sews it and re-drapes it in your browser as the arms move.",
  alternates: { canonical: "/studies/pose/" },
};

export default function PosePage() {
  return <PoseRoom />;
}
