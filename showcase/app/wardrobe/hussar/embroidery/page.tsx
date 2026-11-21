import type { Metadata } from "next";
import HussarStitchLab from "../../../../components/hussar/HussarStitchLab";

export const metadata: Metadata = {
  title: "Trace embroidery onto the hussar dolman · FrankenTextiles",
  description:
    "Follow the exported Tajima DST placement and soutache couching runs onto the solved 3D 1761 Swedish hussar dolman chest through registered panel coordinates.",
  alternates: { canonical: "/wardrobe/hussar/embroidery/" },
};

export default function HussarEmbroideryPage() {
  return <HussarStitchLab />;
}
