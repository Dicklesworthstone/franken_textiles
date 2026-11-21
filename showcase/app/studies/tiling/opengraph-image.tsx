import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("tiling", 630);
export default function Image() { return imageFile("tiling", 630); }
