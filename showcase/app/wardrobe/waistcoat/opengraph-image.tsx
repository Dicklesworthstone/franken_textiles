import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("waistcoat", 630);
export default function Image() { return imageFile("waistcoat", 630); }
