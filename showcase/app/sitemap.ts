import type { MetadataRoute } from "next";
import { siteOrigin, workbenches } from "../components/site/links";
import { sections } from "../share/catalog";
import { experiments } from "../components/materialExperiments";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const page = (path: string, priority: number): MetadataRoute.Sitemap[number] => ({ url: siteOrigin + path, changeFrequency: "weekly", priority });
  return [
    page("/", 1),
    page("/how-it-works/", .9),
    page("/get-started/", .9),
    page("/wardrobe/sundress/", .8),
    page("/wardrobe/sundress/redraft/", .7),
    page("/wardrobe/hussar/", .8),
    page("/wardrobe/hussar/embroidery/", .7),
    page("/wardrobe/waistcoat/", .8),
    page("/wardrobe/waistcoat/inside-out/", .7),
    page("/studies/trousers/", .8),
    page("/studies/skirt/", .9),
    page("/studies/wrap-skirt/", .7),
    page("/studies/tiling/", .7),
    page("/studies/sewing/", .7),
    ...Object.keys(experiments).map(kind => page(`/experiments/${kind}/`, .7)),
    page("/experiments/thermal/", .7),
    page("/experiments/wash/", .7),
    page("/experiments/friction/", .7),
    ...workbenches.map(item => page(item.href, .7)),
    ...sections.map(section => page(`/sections/${section}/`, .5)),
  ];
}
