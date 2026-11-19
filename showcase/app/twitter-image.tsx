import { imageFile, imageMetadata } from "../share/image";
export const generateImageMetadata = () => imageMetadata("home",600);
export default function Image() { return imageFile("home",600); }
