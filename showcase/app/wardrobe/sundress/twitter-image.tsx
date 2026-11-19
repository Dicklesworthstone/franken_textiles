import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("sundress",600);
export default function Image() { return imageFile("sundress",600); }
