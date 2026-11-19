import type { Metadata, Viewport } from "next";
import "@fontsource/dm-sans/latin-400.css";
import "@fontsource/dm-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-500.css";
import "@fontsource/instrument-serif/latin-400.css";
import "@fontsource/instrument-serif/latin-400-italic.css";
import "./style.css";
import { repo, siteOrigin } from "../components/site/links";

const description = "Change the cut of a six-panel skirt on a fitted adult form. Explore native cloth physics, pose a tailored bolero and print the full-size sewing patterns.";

export const metadata: Metadata = {
  title: "FrankenTextiles — The Pattern Room",
  description,
  metadataBase: new URL(siteOrigin),
  applicationName: "FrankenTextiles",
  authors: [{ name: "Jeffrey Emanuel" }],
  openGraph: { siteName: "FrankenTextiles", type: "website", locale: "en_US" },
  twitter: { card: "summary_large_image" },
  formatDetection: { telephone: false },
};

export const viewport: Viewport = { themeColor: "#f7f4ed", colorScheme: "light" };

// Describes the project to search engines. It states only what the repository itself states.
const structured = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "FrankenTextiles",
  url: siteOrigin,
  description: "Makes sewing patterns that demonstrably sew together and fit a specific body, and writes the files people and machines need to make them.",
  applicationCategory: "DesignApplication",
  operatingSystem: "Any modern web browser; command-line tool built from source with Rust",
  offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  codeRepository: repo,
  license: `${repo}/blob/main/LICENSE`,
  author: { "@type": "Person", name: "Jeffrey Emanuel" },
};

export default function Layout({ children }: { children: React.ReactNode }) {
  return <html lang="en"><body>
    {children}
    <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(structured).replace(/</g, "\\u003c") }}/>
  </body></html>;
}
