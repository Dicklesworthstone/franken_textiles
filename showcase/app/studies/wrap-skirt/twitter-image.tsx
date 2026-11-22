import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("wrapSkirt", 600);
export default function Image() { return imageFile("wrapSkirt", 600); }
