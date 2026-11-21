import type { Metadata } from "next";
import TileFloorLab from "../../../components/TileFloorLab";

export const metadata: Metadata = {
  title: "Assemble an A4 pattern · FrankenTextiles",
  description:
    "Inspect a real exported tiled A4 sewing pattern on a virtual cutting-room floor: toggle alignment crosses and join diamonds, compare against the full uncut master and 100 mm scale bar, and download the exact PDF.",
  alternates: { canonical: "/studies/tiling/" },
};

export default function TilingPage() {
  return <TileFloorLab />;
}
