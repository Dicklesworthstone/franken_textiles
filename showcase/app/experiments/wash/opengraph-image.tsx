import { imageFile } from "../../../share/image";
export const dynamic = "force-static";
export const alt = "Wash, shrink and recut a garment · FrankenTextiles";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export default function Image() { return imageFile("wash", 630); }
