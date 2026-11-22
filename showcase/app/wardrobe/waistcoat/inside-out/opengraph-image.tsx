import { imageFile, imageMetadata } from "../../../../share/image";
export const generateImageMetadata = () => imageMetadata("waistcoatInsideOut", 630);
export default function Image() { return imageFile("waistcoatInsideOut", 630); }
