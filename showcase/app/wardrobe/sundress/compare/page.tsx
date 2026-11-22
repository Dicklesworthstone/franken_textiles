import type { Metadata } from "next";
import SundressCompareLab from "../../../../components/sundress/SundressCompareLab";
import { siteName } from "../../../../components/site/links";

export const metadata: Metadata = {
  title: `Compare two native drapes of one fixed dress — ${siteName}`,
  description:
    "Inspect independently accepted native drapes of one fixed Garden Sundress on two synthetic adult forms under one synchronized camera and common world scale, plus explicit solver refusals.",
  alternates: { canonical: "/wardrobe/sundress/compare/" },
};

export default function SundressComparePage() {
  return <SundressCompareLab />;
}
