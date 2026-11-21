import type { Metadata } from "next";
import WashShrinkLab from "../../../components/WashShrinkLab";

export const metadata: Metadata = {
  title: "Wash, shrink & recut · FrankenTextiles",
  description:
    "Apply source-qualified ISO 6330 laundering shrinkage from Kanat & Ozdil (2026) along each pattern piece's grainline, inspect post-wash seam shifts, or compute an oversized pre-wash compensation cut in local Rust WebAssembly.",
  alternates: { canonical: "/experiments/wash/" },
};

export default function WashPage() {
  return <WashShrinkLab />;
}
