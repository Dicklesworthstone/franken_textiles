import type { MetadataRoute } from "next";

export const dynamic = "force-static";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "FrankenTextiles — The Pattern Room",
    short_name: "FrankenTextiles",
    description: "Sewing patterns that demonstrably sew together and fit a specific body. Draft, check, drape and export on your own device.",
    start_url: "/",
    display: "minimal-ui",
    background_color: "#f7f4ed",
    theme_color: "#f7f4ed",
    icons: [
      { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png" },
      { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png" },
    ],
  };
}
