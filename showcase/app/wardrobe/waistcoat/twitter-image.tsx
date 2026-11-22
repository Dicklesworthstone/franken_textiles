import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("waistcoat", 600);
export default function Image() { return imageFile("waistcoat", 600); }
