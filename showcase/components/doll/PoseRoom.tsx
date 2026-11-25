"use client";

import dynamic from "next/dynamic";
import { useCallback, useEffect, useRef, useState } from "react";
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

function phaseLine(status: PoseStatus, elapsed: number) {
  switch (status.phase) {
    case "loading": return "Loading the form";
    case "sewing": return `Sewing the jacket on in this pose · ${elapsed.toFixed(0)} s`;
    case "moving": return `Moving the arm under the jacket · ${elapsed.toFixed(0)} s`;
    case "replay": return "Replaying what the solver did";
    case "failed": return "The solver could not start";
    default: return "Ready: drag an arm";
  }
}

export default function PoseRoom() {
  const [status, setStatus] = useState<PoseStatus>({ phase: "loading", lines: [], angles: [], shown: [35, 35] });
  const [elapsed, setElapsed] = useState(0);
  const command = useRef<((c: PoseCommand) => void) | null>(null);
  const onStatus = useCallback((next: PoseStatus) => setStatus(next), []);
  useEffect(() => {
    if ((status.phase !== "sewing" && status.phase !== "moving") || status.sewingSince === undefined) return;
    const since = status.sewingSince;
    // Start from this run's own time at once, not the last run's until the first tick.
    const tick = () => setElapsed((performance.now() - since) / 1000);
    tick();
    const timer = setInterval(tick, 250);
    return () => clearInterval(timer);
  }, [status.phase, status.sewingSince]);
  const { angles } = status;
  const ready = angles.length > 0 && status.phase !== "failed";
  const index = (a: number) => Math.max(0, angles.findIndex(x => x >= a - 0.5));
  const arms = (left: number, right: number) => command.current?.({ kind: "arms", left, right });

  return <>
    <SiteHeader />
    <main id="main" className="pose-page">
      <Crumbs trail={[["Exhibition", "/"], ["Drape gallery", "/sections/studies/"], ["Pose the figure"]]} />
      <div className="wrap">
        <header className="pose-heading">
          <p className="pose-kicker">Live in your browser</p>
          <h1>Pose the arms. <em>The jacket is sewn on again.</em></h1>
          <p className="pose-lead">Drag an arm to where you want it. When you let go, the project&rsquo;s own cloth
            solver, running in this tab, lays the five pieces of the bolero round the form in that pose, sews the
            seams and lets the jacket hang. Nudge an arm by a few degrees instead and the solver carries the sewn
            jacket along with it. Nothing is recorded in advance.</p>
        </header>

        <div className="pose-bench">
          <figure className="pose-stage" data-phase={status.phase}>
            <PoseScene onStatus={onStatus} command={command} />
            <figcaption className="pose-state" role="status" aria-live="polite">
              <span className="pose-dot" aria-hidden="true" />{phaseLine(status, elapsed)}
            </figcaption>
            {status.phase === "sewing" && <div className="pose-progress" aria-hidden="true">
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
              {presets.map(([label, left, right]) => <button key={label} type="button" disabled={!ready}
                aria-pressed={status.shown[0] === left && status.shown[1] === right}
                onClick={() => arms(left, right)}>{label}</button>)}
            </div>
            {(["Left", "Right"] as const).map((name, side) => <label className="pose-slider" key={name}>
              <span>{name} arm <b>{Math.round(status.shown[side])}°</b></span>
              <input type="range" min={0} max={Math.max(0, angles.length - 1)} step={1} disabled={!ready}
                value={index(status.shown[side])}
                aria-valuetext={`${Math.round(status.shown[side])} degrees`}
                onChange={e => {
                  const a = angles[Number(e.target.value)];
                  if (side === 0) arms(a, status.shown[1]); else arms(status.shown[0], a);
                }} />
            </label>)}
            <p className="pose-note">Angles are measured from the arm hanging straight down. Each arm has
              {angles.length ? ` ${angles.length} ` : " "}poses from {angles[0] ?? 35}° to {angles[angles.length - 1] ?? 135}°.</p>
            {status.message && status.phase !== "failed" && <p className="pose-refused" role="alert">{status.message}</p>}

            <dl className="pose-readout">
              <div><dt>Last sewn</dt><dd>{status.sewSeconds === undefined ? "…" :
                `${status.clothVertices?.toLocaleString("en-GB")} cloth points, sewn and hung in ${status.sewSeconds.toFixed(1)} s in this tab`}</dd></div>
              {status.moveSeconds !== undefined && <div><dt>Last move</dt><dd>
                {`${status.moveSubsteps} checked steps, solved in ${status.moveSeconds.toFixed(1)} s in this tab`}</dd></div>}
              <div><dt>Pose of the jacket</dt><dd>{status.sewn ? `left ${Math.round(status.sewn[0])}° · right ${Math.round(status.sewn[1])}°` : "…"}</dd></div>
              <div><dt>Solver warnings</dt><dd>{status.flags === undefined ? "…" : status.flags.length === 0 ? "None for this drape" :
                <ul className="pose-flags">{status.flags.map(f => <li key={f}>{f}</li>)}</ul>}</dd></div>
            </dl>
            {!!status.flags?.length && <p className="pose-note">These are the solver&rsquo;s own words. The
              thickness ramp is short when cloth is pinched somewhere, usually under an arm, so it sits closer
              to the form there than the cloth&rsquo;s full thickness. &ldquo;Not numerically settled&rdquo; means
              the jacket was still moving a few millimetres a second when the run stopped.</p>}
          </aside>
        </div>

        <section className="pose-notes" aria-labelledby="pose-how">
          <h2 id="pose-how">What you are looking at</h2>
          <ol>
            <li><b>The jacket is a real drape.</b> For every pose, the solver places the five flat pieces round
              the form, closes the seams and lets the jacket hang under gravity, with contact and friction
              against the form. The replay is its own record of that, frame by frame.</li>
            <li><b>A small move carries the jacket; a larger one sews it again.</b> Dragging moves the form. If you
              let go within 10° of where the jacket is now (the arm&rsquo;s sampled poses are 10° to 15°
              apart), the solver moves the arm there under the jacket in short steps: each starts with the
              cloth near the form moved along with it and is accepted only after the solver&rsquo;s collision
              checks, and the replay shows those steps. Every step but the last stops at a looser solver
              tolerance, which saved a fifth to a quarter of a move&rsquo;s time in our tests; the last meets the
              usual tolerance. Further than that, a new drape starts in the nearest of the arm&rsquo;s sampled poses, which is the quicker of the two
              for a large change; if you move again before it finishes, that drape is dropped. In our tests
              (headless Chrome on an 8-core Linux machine, six solver threads, timing the solver alone) a fresh
              sew took about 11 s, a 5° move 3 to 4 s and a 10° move about 7 s.</li>
            <li><b>It runs on your device.</b> The solver is compiled to WebAssembly and works in background
              threads of this tab. Where the browser allows the threads to share memory and the device has
              cores to spare, the per-element and per-contact work and the sparse factorization run on several
              at once, with the same result to the bit; elsewhere it runs on one. How long a pose takes depends
              on your computer.</li>
          </ol>
          <p className="pose-fine">The form is a synthetic child with a 52 cm chest, made from published tape
            measurements and cut below the waist like a tailor&rsquo;s form. Below 35° the form&rsquo;s own
            surface would cross itself, which the solver refuses, so the arms start there. The cloth uses the
            solver&rsquo;s default fabric values and a 16 mm mesh: this shows how the pattern sits in each pose,
            not how a particular wool would, and a sewn sample is still the test of fit.{" "}
            <a href={repo + "/blob/main/crates/ftx-wasm/src/session.rs"}>Read the session code</a> ·
            <a href="/sections/studies/#drape-room"> See the recorded drapes</a></p>
        </section>
      </div>
    </main>
    <SiteFooter />
  </>;
}
