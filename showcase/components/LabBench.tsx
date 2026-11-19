"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState } from "react";

const DraftLab = dynamic(() => import("./DraftLab"));
const CutLab = dynamic(() => import("./CutLab"));
const StitchLab = dynamic(() => import("./StitchLab"));

const stations = [
  { id: "draft", name: "Pattern drafting", short: "Draft", action: "Change the silhouette" },
  { id: "cut", name: "Roll layout", short: "Cut", action: "Arrange the cut pieces" },
  { id: "stitch", name: "Machine embroidery", short: "Stitch", action: "Follow the needle" },
] as const;

export default function LabBench() {
  const [current, setCurrent] = useState(0);
  const [draft, setDraft] = useState<{ program: string; label: string; pattern: string }>();
  const [transfer, setTransfer] = useState(0);
  const [visited, setVisited] = useState<number[]>([]);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  function activate(idx: number) {
    setVisited(previous => previous.includes(idx) ? previous : [...previous, idx]);
    setCurrent(idx);
  }
  useEffect(() => {
    const restore = () => {
      const requested = new URLSearchParams(location.search).get("lab");
      const idx = stations.findIndex(s => s.id === requested);
      activate(idx < 0 ? 0 : idx);
    };
    restore();
    window.addEventListener("popstate", restore);
    return () => window.removeEventListener("popstate", restore);
  }, []);

  function select(idx: number) {
    activate(idx);
    const url = new URL(location.href);
    url.searchParams.set("lab", stations[idx].id);
    url.hash = "studio";
    history.replaceState(history.state, "", url);
  }
  const { id } = stations[current];
  return <section id="studio" className="paper studio-bench"><div className="wrap">
    <div className="studio-intro"><div><p className="kicker">The worktable</p><h2>Hands-on studios<span>.</span></h2></div>
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
        <img src={`/media/lab-${s.id}.webp`} alt="" width="112" height="86" loading="lazy"/>
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
  </div></section>;
}
