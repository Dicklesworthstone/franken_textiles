import { imageFile } from "../../../share/image";
export const dynamic = "force-static";
export const alt = "Assemble an A4 sewing pattern on screen · FrankenTextiles";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export default function Image() { return imageFile("tiling", 630); }
