import { imageFile } from "../../../share/image";
export const dynamic = "force-static";
export const alt = "Play the actual sewing order on a jacket · FrankenTextiles";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export default function Image() { return imageFile("sewingOrder", 630); }
