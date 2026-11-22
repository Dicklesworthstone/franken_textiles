import type { Metadata } from "next";
import WaistcoatLab from "../../../components/WaistcoatLab";
import { siteName } from "../../../components/site/links";

export const metadata: Metadata = {
  title: `1780s Waistcoat (V&A 256-1880) — ${siteName}`,
  description:
    "Reconstructed 1780s men's waistcoat with separate silk fronts, plain silk facings, linen linings, pocket welts, V&A 256-1880 provenance, and full-size SVG/PDF exports.",
  alternates: { canonical: "/wardrobe/waistcoat/" },
};

export default function WaistcoatPage() {
  return <WaistcoatLab />;
}
