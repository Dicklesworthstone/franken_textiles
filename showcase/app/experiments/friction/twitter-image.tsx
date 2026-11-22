import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("friction", 600);
export default function Image() { return imageFile("friction", 600); }
