import type { ReactNode } from "react";
import SiteHeader from "./SiteHeader";
import SiteFooter from "./SiteFooter";
import Reveal from "./Reveal";

// The shared chrome for a standalone page: header, the page's own content, footer.
export default function SitePage({ children, className = "ft-page" }: { children: ReactNode; className?: string }) {
  return <>
    <SiteHeader/>
    <main id="main" className={className}>{children}</main>
    <SiteFooter/>
    <Reveal/>
  </>;
}
