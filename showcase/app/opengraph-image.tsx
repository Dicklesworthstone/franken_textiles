import { imageFile, imageMetadata } from "../share/image";
export const generateImageMetadata = () => imageMetadata("home",630);
export default function Image() { return imageFile("home",630); }
