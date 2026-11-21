import type { Metadata } from "next";
import SewingOrderLab from "../../../components/SewingOrderLab";

export const metadata: Metadata = {
  title: "Play the actual sewing order · FrankenTextiles",
  description:
    "Step through all ten native sewing-planner operations for the Zouave bolero on flat cut pieces and the 3D jacket, with prerequisites, physical endpoints, and downloadable stitch program.",
  alternates: { canonical: "/studies/sewing/" },
};

export default function SewingOrderPage() {
  return <SewingOrderLab />;
}
