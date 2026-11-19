import type { Metadata } from "next";
import "@fontsource/dm-sans/latin-400.css";
import "@fontsource/dm-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-500.css";
import "@fontsource/instrument-serif/latin-400.css";
import "@fontsource/instrument-serif/latin-400-italic.css";
import "./style.css";

export const metadata: Metadata = {
  title: "FrankenTextiles — The Pattern Room",
  description: "A hands-on sewing studio. Draft a pattern, plan a cutting layout, follow an embroidery needle, and inspect garments sewn in three dimensions.",
  metadataBase: new URL(process.env.FTX_SITE_ORIGIN ?? "https://franken-textiles.com"),
};

export default function Layout({ children }: { children: React.ReactNode }) {
  return <html lang="en"><body>{children}</body></html>;
}
