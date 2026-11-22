import type { Metadata } from "next";
import InsideOutLab from "../../../../components/InsideOutLab";
import { siteName } from "../../../../components/site/links";

export const metadata: Metadata = {
  title: `Turn a lined garment inside out — ${siteName}`,
  description:
    "Rotate the shell, plain silk facing, linen lining, and pocket bags of V&A 256-1880 together, separate layers without changing seam indices, and audit missing collar linings honestly.",
  alternates: { canonical: "/wardrobe/waistcoat/inside-out/" },
};

export default function InsideOutPage() {
  return <InsideOutLab />;
}
