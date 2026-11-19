import type { Metadata } from "next";
import Exhibition from "../components/Exhibition";

export const metadata: Metadata = { alternates: { canonical: "/" } };

export default function Home() {
  return <Exhibition/>;
}
