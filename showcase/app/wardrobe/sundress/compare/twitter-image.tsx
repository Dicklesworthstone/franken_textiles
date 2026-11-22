import { imageFile, imageMetadata } from "../../../../share/image";
export const generateImageMetadata = () => imageMetadata("sundressCompare", 600);
export default function Image() { return imageFile("sundressCompare", 600); }
