"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import { exhibits, homeSections, primary, project, repo, workbenches } from "./links";
import "./site.css";

export function Mark() {
  return <span className="mark" aria-hidden="true"><i/><i/><i/><i/></span>;
}

function current(pathname: string, href: string, inSheet = false) {
  if (href.startsWith("/#")) {
    const anchor = href.slice(2);
    // In the full menu sheet, Skirt atelier, Trouser study, A4 tile assembly, Jacket sewing order and Drape gallery each have their own exact entries.
    if (anchor === "drape-room") return !inSheet && pathname !== "/studies/tiling/" && pathname !== "/studies/sewing/" && (pathname.startsWith("/studies/") || pathname === "/sections/studies/");
    if (anchor === "wardrobe") {
      return inSheet
        ? pathname === "/sections/wardrobe/" || pathname === "/wardrobe/sundress/" || pathname === "/wardrobe/hussar/"
        : pathname.startsWith("/wardrobe/") || pathname === "/sections/wardrobe/";
    }
    if (anchor === "materials") {
      return inSheet
        ? pathname === "/sections/materials/" || pathname === "/experiments/strip/" || pathname === "/experiments/cylinder/"
        : pathname.startsWith("/experiments/") || pathname === "/sections/materials/";
    }
    if (anchor === "studio") {
      return ["studio", "draft", "cut", "stitch", "repair"].some(id => pathname === `/sections/${id}/`) ||
        (!inSheet && (pathname === "/studies/tiling/" || pathname === "/studies/sewing/"));
    }
    return pathname === `/sections/${anchor}/`;
  }
  return pathname === href;
}

export default function SiteHeader({ skipTo = "#main" }: { skipTo?: string }) {
  const pathname = usePathname() ?? "/";
  const [stuck, setStuck] = useState(false);
  const [open, setOpen] = useState(false);
  const sheet = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const onScroll = () => setStuck(window.scrollY > 6);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const el = sheet.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  const mark = (href: string, inSheet = false) => current(pathname, href, inSheet) ? "page" as const : undefined;
  return <>
    <a className="skip" href={skipTo}>Skip to content</a>
    <header className="ft-head" data-stuck={stuck || undefined}>
      <div className="wrap ft-bar">
        <a className="wordmark" href="/" aria-label="FrankenTextiles home"><Mark/>Franken<span>Textiles</span></a>
        <nav className="ft-nav" aria-label="Main navigation">
          {/* The bar names five rooms; this opens the whole catalogue, on any width. */}
          <button type="button" className="ft-index" aria-haspopup="dialog" aria-expanded={open}
            aria-label={`Index of all ${exhibits.length} exhibits`} onClick={() => setOpen(true)}>
            <span className="ft-index-dots" aria-hidden="true"><i/><i/><i/><i/><i/><i/><i/><i/><i/></span>Index<small aria-hidden="true">{exhibits.length}</small>
          </button>
          {primary.map(item => <a key={item.href} href={item.href} aria-current={mark(item.href)}>{item.label}</a>)}
          <span className="ft-nav-rule ft-nav-extra" aria-hidden="true"/>
          {project.map(item => <a key={item.href} className="ft-nav-extra" href={item.href} aria-current={mark(item.href)}>{item.label}</a>)}
        </nav>
        <div className="ft-actions">
          <a className="ft-source" href={repo} aria-label="Source on GitHub">
            <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M8 0a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38v-1.33c-2.23.48-2.7-1.07-2.7-1.07-.36-.93-.89-1.17-.89-1.17-.73-.5.05-.49.05-.49.8.06 1.23.83 1.23.83.72 1.23 1.88.87 2.34.67.07-.52.28-.87.5-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.22 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.19c0 .21.15.46.55.38A8 8 0 0 0 8 0Z"/></svg>
          </a>
          <a className="ft-cta" href="/#workbenches">Workbenches <span aria-hidden="true" className="arrow arrow-diagonal"/></a>
          <button type="button" className="ft-menu" aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(true)}>
            <span aria-hidden="true"><i/><i/></span>Menu
          </button>
        </div>
      </div>
      <span className="ft-thread" aria-hidden="true"/>
    </header>
    {/* A link's label is a child element, so the click usually lands on that child, not on the anchor. An entry
        that points into the page being read does not leave it, so the sheet has to be closed here. */}
    <dialog ref={sheet} className="ft-sheet" aria-label="Site menu" onClose={() => setOpen(false)}
      onClick={event => { if (event.target instanceof Element && event.target.closest("a")) setOpen(false); }}>
      <div className="ft-sheet-top wrap">
        <a className="wordmark" href="/"><Mark/>Franken<span>Textiles</span></a>
        <button type="button" className="ft-close" onClick={() => setOpen(false)}>Close <span aria-hidden="true">×</span></button>
      </div>
      {/* The home page is long. Where the page rail is not drawn, its sections are listed here first. */}
      {pathname === "/" && <nav className="wrap ft-sheet-here" aria-labelledby="sheet-here">
        <h2 id="sheet-here" className="mono">On this page</h2>
        <ol>{homeSections.map(([id, label]) => <li key={id}><a href={`#${id}`}>{label}</a></li>)}</ol>
      </nav>}
      <div className="wrap ft-sheet-body">
        <section aria-labelledby="sheet-exhibits"><h2 id="sheet-exhibits" className="mono">The exhibition</h2>
          {/* Each still is fetched only once the sheet is shown: a closed dialog is not rendered. */}
          <ul>{exhibits.map(item => <li key={item.href}><a href={item.href} aria-current={mark(item.href, true)}>
            {item.thumb && <img src={`/media/index/${item.thumb}.webp`} alt="" width={330} height={220} loading="lazy" decoding="async"/>}
            <strong>{item.label}</strong><small>{item.note}</small></a></li>)}</ul>
        </section>
        <section aria-labelledby="sheet-tools"><h2 id="sheet-tools" className="mono">Workbenches · run on your device</h2>
          <ul>{workbenches.map(item => <li key={item.href}><a href={item.href}><strong>{item.label}</strong><small>{item.note}</small></a></li>)}</ul>
        </section>
        <section aria-labelledby="sheet-project"><h2 id="sheet-project" className="mono">The project</h2>
          <ul>{project.map(item => <li key={item.href}><a href={item.href} aria-current={mark(item.href, true)}><strong>{item.label}</strong><small>{item.note}</small></a></li>)}
            <li><a href={repo}><strong>Source on GitHub</strong><small>Rust, MIT licence with rider</small></a></li></ul>
        </section>
      </div>
    </dialog>
  </>;
}
