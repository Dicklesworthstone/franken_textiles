import { sections } from "../../../share/catalog";

export const dynamicParams = false;
export const generateStaticParams = () => sections.map(section => ({section}));
export default function Layout({children}: {children: React.ReactNode}) { return children; }
