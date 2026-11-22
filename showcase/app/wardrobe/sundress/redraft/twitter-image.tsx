import { imageFile, imageMetadata } from "../../../../share/image";
export const generateImageMetadata = () => imageMetadata("sundressRedraft", 600);
export default function Image() { return imageFile("sundressRedraft", 600); }
