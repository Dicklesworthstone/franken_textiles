import type { Metadata } from "next";
import Exhibition from "../components/Exhibition";
import SectionRail from "../components/site/SectionRail";

export const metadata: Metadata = { alternates: { canonical: "/" } };

export default function Home() {
  return <><Exhibition/><SectionRail/></>;
}
