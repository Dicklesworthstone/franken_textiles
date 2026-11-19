import { imageFile, imageMetadata } from "../../share/image";
export const generateImageMetadata = () => imageMetadata("getStarted",600);
export default function Image() { return imageFile("getStarted",600); }
