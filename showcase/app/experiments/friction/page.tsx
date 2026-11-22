import type { Metadata } from "next";
import FrictionSledLab from "../../../components/FrictionSledLab";
import { siteName } from "../../../components/site/links";

export const metadata: Metadata = {
  title: `Friction sled · Stick-to-slip reactions — ${siteName}`,
  description:
    "Step through native FrankenTextiles sliding-sled trajectories between two synthetic surface states, inspect retained normal and tangential reactions across the Coulomb stick-to-slip threshold, and verify live Rust WebAssembly friction calibration.",
  alternates: { canonical: "/experiments/friction/" },
};

export default function FrictionSledPage() {
  return <FrictionSledLab />;
}
