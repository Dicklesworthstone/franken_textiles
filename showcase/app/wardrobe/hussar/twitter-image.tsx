import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("hussar",600);
export default function Image() { return imageFile("hussar",600); }
