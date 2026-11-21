"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState } from "react";
import type { CutDraft } from "./DraftLab";

const DraftLab = dynamic(() => import("./DraftLab"));
const CutLab = dynamic(() => import("./CutLab"));
const StitchLab = dynamic(() => import("./StitchLab"));
const RepairLab = dynamic(() => import("./RepairLab"));

const stations = [
  { id: "draft", name: "Pattern drafting", short: "Draft", action: "Change the silhouette", thumb: "/media/lab-draft.webp" },
  { id: "cut", name: "Roll layout", short: "Cut", action: "Arrange the cut pieces", thumb: "/media/lab-cut.webp" },
  { id: "stitch", name: "Machine embroidery", short: "Stitch", action: "Follow the needle", thumb: "/media/lab-stitch.webp" },
  { id: "repair", name: "Seam repair", short: "Repair", action: "Fix a mismatched seam", thumb: "/media/construction-bolero-cut.jpg" },
] as const;

export default function LabBench({initialStation,heading:Heading="h2"}: {initialStation?: "draft" | "cut" | "stitch" | "repair";heading?:"h1"|"h2"}) {
  const initial = Math.max(0, stations.findIndex(s => s.id === initialStation));
  const [current, setCurrent] = useState(initial);
  const [draft, setDraft] = useState<CutDraft>();
  const [transfer, setTransfer] = useState(0);
  const [visited, setVisited] = useState<number[]>([]);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  function activate(idx: number) {
    setVisited(previous => previous.includes(idx) ? previous : [...previous, idx]);
    setCurrent(idx);
  }
  useEffect(() => {
    const restore = () => {
      const requested = new URLSearchParams(location.search).get("lab") ?? location.pathname.split("/")[2];
      const idx = stations.findIndex(s => s.id === requested);
      activate(idx < 0 ? initial : idx);
    };
    restore();
    window.addEventListener("popstate", restore);
    return () => window.removeEventListener("popstate", restore);
  }, [initial]);

  function select(idx: number) {
    activate(idx);
    const url = new URL(location.href);
    if (url.pathname.startsWith("/sections/")) {
      url.pathname = "/sections/" + stations[idx].id + "/";
      url.searchParams.delete("lab");
    } else {
      url.searchParams.set("lab", stations[idx].id);
      url.hash = "studio";
    }
    history.replaceState(history.state, "", url);
  }
  const { id } = stations[current];
  return <section id="studio" className="paper studio-bench"><div className="wrap">
    <div className="studio-intro"><div><p className="kicker">The worktable</p><Heading className="section-title"><a href="/sections/studio/">Hands-on studios<span>.</span></a></Heading></div>
      <p>Change a parameter. Inspect the result. Take the actual pattern, cutting marker, or stitch file with you.</p></div>
    <div className="station-tabs" role="tablist" aria-label="Choose a studio">
      {stations.map((s, i) => <button key={s.id} role="tab" id={`tab-${s.id}`}
        ref={el => { tabs.current[i] = el; }} tabIndex={current === i ? 0 : -1}
        aria-selected={current === i} aria-controls={`studio-${s.id}`} onClick={() => select(i)}
        onKeyDown={e => {
          const next = e.key === "ArrowRight" ? (i + 1) % stations.length :
            e.key === "ArrowLeft" ? (i + stations.length - 1) % stations.length :
            e.key === "Home" ? 0 : e.key === "End" ? stations.length - 1 : undefined;
          if (next !== undefined) { e.preventDefault(); select(next); tabs.current[next]?.focus(); }
        }}>
        <img src={s.thumb} alt="" width="112" height="86" loading="lazy"/>
        <span><small>{s.action}</small><strong className="station-long">{s.name}</strong><strong className="station-short">{s.short}</strong></span>
        <b aria-hidden="true">{current === i ? "↓" : "↗"}</b>
      </button>)}
    </div>
    <div id="studio-draft" className="station-body" role="tabpanel" aria-labelledby="tab-draft" hidden={id !== "draft"}>
      <DraftLab enabled={visited.includes(0)} onPlanCut={result => {
        setDraft(result);
        setTransfer(key => key + 1);
        select(1);
        tabs.current[1]?.focus({ preventScroll: true });
        tabs.current[1]?.scrollIntoView({ block: "nearest" });
      }}/>
    </div>
    <div id="studio-cut" className="station-body" role="tabpanel" aria-labelledby="tab-cut" hidden={id !== "cut"}>
      {visited.includes(1) && <CutLab key={transfer} draft={draft}/>}
    </div>
    <div id="studio-stitch" className="station-body" role="tabpanel" aria-labelledby="tab-stitch" hidden={id !== "stitch"}>
      {visited.includes(2) && <StitchLab active={id === "stitch"}/>}
    </div>
    <div id="studio-repair" className="station-body" role="tabpanel" aria-labelledby="tab-repair" hidden={id !== "repair"}>
      {visited.includes(3) && <RepairLab/>}
    </div>
    <a className="station-permalink" href={`/sections/${id}/`}>Link to this studio ↗</a>
  </div></section>;
}
