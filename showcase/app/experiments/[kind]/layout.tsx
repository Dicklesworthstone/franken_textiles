import { experiments } from "../../../components/materialExperiments";

export const dynamicParams = false;
export const generateStaticParams = () => Object.keys(experiments).map(kind => ({kind}));
export default function Layout({children}: {children: React.ReactNode}) { return children; }
