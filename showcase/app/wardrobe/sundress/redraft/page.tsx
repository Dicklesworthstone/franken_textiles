import type { Metadata } from "next";
import SundressRedraftLab from "../../../../components/sundress/SundressRedraftLab";
import { siteName } from "../../../../components/site/links";

export const metadata: Metadata = {
  title: `Redraft the sundress after an ease warning — ${siteName}`,
  description:
    "Request a second checked Garden Sundress pattern in local Rust WebAssembly after a bust or waist ease warning, pin both silhouettes at 1:1 scale, and inspect recalculated seam lengths.",
  alternates: { canonical: "/wardrobe/sundress/redraft/" },
};

export default function SundressRedraftPage() {
  return <SundressRedraftLab />;
}
