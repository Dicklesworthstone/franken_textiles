import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("wash", 600);
export default function Image() { return imageFile("wash", 600); }
