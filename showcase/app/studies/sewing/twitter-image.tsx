import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("sewingOrder", 600);
export default function Image() { return imageFile("sewingOrder", 600); }
