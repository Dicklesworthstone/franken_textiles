import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("friction", 630);
export default function Image() { return imageFile("friction", 630); }
