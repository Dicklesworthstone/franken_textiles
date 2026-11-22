"use client";

import { useEffect, useState } from "react";
import { homeSections as stops } from "./links";
import "./site.css";

// The home page runs to about eighteen screens. This is a tailor's tape down its left margin: one mark for
// each named section and a longer one for the section being read. Under the pointer or the keyboard it
// opens into a short table of contents. It is drawn only on a wide screen with a pointer; everywhere else
// the menu lists the same sections under "On this page", so nothing here is the only way to reach one.
const number = (index: number) => String(index + 1).padStart(2, "0");

export default function SectionRail() {
  const [current, setCurrent] = useState(0);
  useEffect(() => {
    // The current section is the last one whose top has passed a line just above the middle of the
    // window. Tops are measured when the page's height changes, not on every scroll, and compared with
    // the scroll position once a frame; an observer would keep a stale answer after a jump to the footer.
    const sections = stops.map(([id]) => document.getElementById(id));
    let tops: number[] = [];
    let frame = 0;
    const update = () => {
      frame = 0;
      const line = scrollY + innerHeight * .45;
      let index = 0;
      tops.forEach((top, i) => { if (top <= line) index = i; });
      setCurrent(index);
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(update); };
    const measure = () => {
      tops = sections.map(section => section ? section.getBoundingClientRect().top + scrollY : Infinity);
      schedule();
    };
    measure();
    const resized = new ResizeObserver(measure);
    resized.observe(document.body);
    addEventListener("scroll", schedule, { passive: true });
    return () => { resized.disconnect(); removeEventListener("scroll", schedule); cancelAnimationFrame(frame); };
  }, []);
  return <nav className="ft-rail" aria-label="On this page">
    <ol>{stops.map(([id, label], index) => <li key={id}>
      <a href={`#${id}`} aria-current={index === current ? "location" : undefined}>
        <i aria-hidden="true"/><span><b>{number(index)}</b>{label}</span>
      </a></li>)}</ol>
    <p className="ft-rail-now" aria-hidden="true"><b>{number(current)}</b> / {stops.length}</p>
  </nav>;
}
