import { imageFile, imageMetadata } from "../../../../share/image";
export const generateImageMetadata = () => imageMetadata("waistcoatInsideOut", 600);
export default function Image() { return imageFile("waistcoatInsideOut", 600); }
