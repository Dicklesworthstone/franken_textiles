import { imageFile } from "../../../share/image";
export const dynamic = "force-static";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const alt = "Measured neoprene and bovine leather steady thermal conduction experiment in FrankenTextiles.";
export default async function Image() { return imageFile("thermal", 630); }
