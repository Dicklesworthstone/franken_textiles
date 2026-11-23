"use client";

import dynamic from "next/dynamic";
import { useCallback, useRef, useState } from "react";
import SiteHeader from "../site/SiteHeader";
import SiteFooter from "../site/SiteFooter";
import Crumbs from "../site/Crumbs";
import type { PoseCommand, PoseStatus } from "./PoseScene";
import "./pose.css";

const PoseScene = dynamic(() => import("./PoseScene"), {
  ssr: false,
  loading: () => <div className="pose-canvas" />,
});

const repo = "https://github.com/Dicklesworthstone/franken_textiles";
const presets: [string, number, number][] = [
  ["Arms down", 35, 35], ["Arms out", 90, 90], ["Arms up", 135, 135], ["One arm up", 135, 35],
];

function phaseLine(status: PoseStatus) {
  switch (status.phase) {
    case "loading": return "Loading the solver";
    case "sewing": return "Sewing the bolero on the form";
    case "replay": return "Replaying the sewing and the drape";
    case "solving": return "Solving the cloth as the arms move";
    case "failed": return "The solver could not start";
    default: return "Ready: drag an arm";
  }
}

export default function PoseRoom() {
  const [status, setStatus] = useState<PoseStatus>({ phase: "loading", lines: [], arms: [35, 35], target: [35, 35] });
  const command = useRef<((c: PoseCommand) => void) | null>(null);
  const onStatus = useCallback((next: PoseStatus) => setStatus(next), []);
  const live = status.phase === "ready" || status.phase === "solving" || status.phase === "replay";
  const [low, high] = status.range ?? [35, 135];
  const arms = (left: number, right: number) => command.current?.({ kind: "arms", left, right });

  return <>
    <SiteHeader />
    <main id="main" className="pose-page">
      <Crumbs trail={[["Exhibition", "/"], ["Drape gallery", "/sections/studies/"], ["Pose the figure"]]} />
      <div className="wrap">
        <header className="pose-heading">
          <p className="pose-kicker">Live in your browser</p>
          <h1>Move the arms. <em>The cloth follows.</em></h1>
          <p className="pose-lead">This bolero is sewn and draped by the project&rsquo;s own cloth solver, running in this tab.
            Drag an arm up or down: the solver moves the form and works the cloth out again as it goes. Nothing is
            recorded in advance.</p>
        </header>

        <div className="pose-bench">
          <figure className="pose-stage" data-phase={status.phase}>
            <PoseScene onStatus={onStatus} command={command} />
            <figcaption className="pose-state" role="status" aria-live="polite">
              <span className="pose-dot" aria-hidden="true" />{phaseLine(status)}
            </figcaption>
            {!live && status.phase !== "failed" && <div className="pose-progress" aria-hidden="true">
              {status.lines.slice(-3).map((line, i) => <code key={i + line}>{line}</code>)}
            </div>}
            {status.phase === "failed" && <div className="pose-failed">
              <p>{status.message}</p>
              <p>The recorded drapes in the <a href="/sections/studies/#drape-room">drape room</a> do not need the live solver.</p>
            </div>}
            <p className="pose-help">Drag an arm to pose it · drag elsewhere to turn · scroll to zoom</p>
          </figure>

          <aside className="pose-panel">
            <h2>Pose</h2>
            <div className="pose-presets" role="group" aria-label="Poses">
              {presets.map(([label, left, right]) => <button key={label} type="button" disabled={!live}
                aria-pressed={Math.abs(status.target[0] - left) < 1 && Math.abs(status.target[1] - right) < 1}
                onClick={() => arms(left, right)}>{label}</button>)}
            </div>
            <label className="pose-slider">
              <span>Left arm <b>{Math.round(status.target[0])}°</b></span>
              <input type="range" min={low} max={high} step={5} value={Math.round(status.target[0])} disabled={!live}
                onChange={e => arms(Number(e.target.value), status.target[1])} />
            </label>
            <label className="pose-slider">
              <span>Right arm <b>{Math.round(status.target[1])}°</b></span>
              <input type="range" min={low} max={high} step={5} value={Math.round(status.target[1])} disabled={!live}
                onChange={e => arms(status.target[0], Number(e.target.value))} />
            </label>
            <p className="pose-note">Angles are measured from the arm hanging straight down.</p>
            <button type="button" className="pose-settle" disabled={!live} onClick={() => command.current?.({ kind: "settle" })}>
              Let the cloth settle</button>
            {status.refused && <p className="pose-refused" role="alert">The solver refused that move, so the cloth stays
              where it was. <small>{status.refused}</small></p>}

            <dl className="pose-readout">
              <div><dt>Opening drape</dt><dd>{status.openingSeconds === undefined ? "…" :
                `${status.clothVertices?.toLocaleString("en-GB")} cloth points sewn and hung in ${status.openingSeconds.toFixed(1)} s`}</dd></div>
              <div><dt>Last move</dt><dd>{status.last ? `${status.last.degrees.toFixed(0)}° over ${(status.last.clothSeconds * 1000).toFixed(0)} ms of cloth time, ${status.last.steps} solver steps, worked out in ${(status.last.milliseconds / 1000).toFixed(1)} s` : "Drag an arm"}</dd></div>
              <div><dt>Arms now</dt><dd>{status.arms.map(a => `${Math.round(a)}°`).join(" left · ")} right</dd></div>
            </dl>
          </aside>
        </div>

        <section className="pose-notes" aria-labelledby="pose-how">
          <h2 id="pose-how">What you are looking at</h2>
          <ol>
            <li><b>The garment is a real drape.</b> When the page opened, the solver laid the five pieces of the bolero
              round the form, sewed the seams and let the jacket hang under gravity, all in this tab.</li>
            <li><b>Each move is solved.</b> The form moves along a short straight path; the solver steps the cloth
              forward with contact and friction at every step. A move it cannot finish cleanly is refused, and the
              cloth stays where it was.</li>
            <li><b>Only the arms move.</b> They go from 35° to 135°. Closer to the body the form&rsquo;s own
              surface would cross itself, and the solver will not start from that.</li>
          </ol>
          <p className="pose-fine">The form is a synthetic child with a 52 cm chest, made from published tape
            measurements and cut below the waist like a tailor&rsquo;s form. The cloth uses the solver&rsquo;s default
            fabric values, so this shows how the pattern moves, not how a particular wool would. A sewn sample is still
            the test of fit. <a href={repo + "/blob/main/crates/ftx-wasm/src/session.rs"}>Read the session code</a> ·
            <a href="/sections/studies/#drape-room"> See the recorded drapes</a></p>
        </section>
      </div>
    </main>
    <SiteFooter />
  </>;
}
