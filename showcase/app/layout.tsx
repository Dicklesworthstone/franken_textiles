import type { Metadata } from "next";
import "@fontsource/dm-sans/latin-400.css";
import "@fontsource/dm-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-500.css";
import "@fontsource/instrument-serif/latin-400.css";
import "@fontsource/instrument-serif/latin-400-italic.css";
import "./style.css";

export const metadata: Metadata = {
  title: "FrankenTextiles — The Drape Studies",
  description: "Explore real sewing patterns in three dimensions. Orbit a draped garment, follow its seams, and replay its journey from pattern board to cloth.",
};

export default function Layout({ children }: { children: React.ReactNode }) {
  return <html lang="en"><body>{children}</body></html>;
}
