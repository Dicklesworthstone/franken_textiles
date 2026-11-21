import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("wash", 630);
export default function Image() { return imageFile("wash", 630); }
