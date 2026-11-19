import { imageFile, imageMetadata } from "../../share/image";
export const generateImageMetadata = () => imageMetadata("howItWorks",630);
export default function Image() { return imageFile("howItWorks",630); }
