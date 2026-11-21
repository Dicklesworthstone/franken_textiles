import { imageFile, imageMetadata } from "../../../share/image";
export const generateImageMetadata = () => imageMetadata("sewingOrder", 630);
export default function Image() { return imageFile("sewingOrder", 630); }
