import type { Metadata } from "next";
import SitePage from "../components/site/SitePage";

export const metadata: Metadata = { title: "Page not found · FrankenTextiles", robots: { index: false } };

export default function NotFound() {
  return <SitePage className="ft-lost">
    <div className="wrap">
      <svg viewBox="0 0 360 262" role="img" aria-label="A pattern piece with a grain line, labelled 404">
        <path d="M62 226V96c0-34 24-52 52-60l42-12c8 30 36 48 76 40l44-9 9 52c-28 8-36 36-31 62v57Z" fill="#efeadd" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round"/>
        <path d="M76 212V98c0-25 18-40 42-47l30-9c13 27 43 43 82 36l33-7 5 29c-26 12-34 40-30 66v46Z" fill="none" stroke="#a24628" strokeWidth="1.3" strokeDasharray="7 5" strokeLinejoin="round"/>
        <path d="M98 96v96m-6-86 6-10 6 10m-12 76 6 10 6-10" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"/>
        <path d="m62 150-9 6 9 6ZM170 226l6 9 6-9Z" fill="#a24628"/>
        <text x="172" y="172" textAnchor="middle" fontFamily="var(--serif)" fontSize="62" letterSpacing="-2.5" fill="currentColor">404</text>
        <text x="180" y="256" textAnchor="middle" fontFamily="var(--mono)" fontSize="8.5" letterSpacing="1.8" fill="#655f52">PIECE NOT IN THE ENVELOPE · CUT 0</text>
      </svg>
      <h1>This piece is <em>missing</em>.</h1>
      <p>The page you asked for is not here. It may have moved, or the link may have been cut a little short.</p>
      <div className="ft-lost-links">
        <a className="primary" href="/">Back to the exhibition <span aria-hidden="true" className="arrow"/></a>
        <a className="ft-text-link" href="/how-it-works/">How it works</a>
        <a className="ft-text-link" href="/get-started/">Get started</a>
      </div>
    </div>
  </SitePage>;
}
