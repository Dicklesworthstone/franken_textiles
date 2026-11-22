import type { Metadata } from "next";
import WrapSkirtLab from "../../../components/WrapSkirtLab";
import { siteName } from "../../../components/site/links";

export const metadata: Metadata = {
  title: `Adjustable-overlap wrap skirt — ${siteName}`,
  description:
    "Hold one checked wrap-skirt pattern fixed while switching synthetic adult forms, neutral vs seated Anny poses, and two adjustable waist-tie settings checked against flat pattern lengths.",
  alternates: { canonical: "/studies/wrap-skirt/" },
};

export default function WrapSkirtPage() {
  return <WrapSkirtLab />;
}
