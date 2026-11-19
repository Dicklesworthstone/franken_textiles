"use client";

import { useEffect, useRef, useState } from "react";

const revisions: Record<string, string> = JSON.parse(process.env.NEXT_PUBLIC_FILM_REVISIONS ?? "{}");
export function filmFile(id: string, ext: "mp4" | "jpg" = "mp4") {
  const name = `${id}.${ext}`;
  return revisions[name] ? `/media/${name}?v=${revisions[name]}` : `/media/${name}`;
}

export function PlaybackMark({playing=false}: {playing?:boolean}) {
  return <span aria-hidden="true" className={playing ? "playback-mark is-playing" : "playback-mark"}/>;
}

export default function InlineFilm({ id, title, controls = false, requested = false }: { id: string; title: string; controls?: boolean; requested?: boolean }) {
  const video = useRef<HTMLVideoElement>(null);
  const toggle = useRef<() => void>(()=>{});
  const [playing,setPlaying] = useState(false);
  // A poster is fetched as soon as it is in the markup, so name it only when the film is about to scroll into view.
  const [near,setNear] = useState(false);
  useEffect(() => {
    const observer=new IntersectionObserver(([entry])=>{ if(entry.isIntersecting){setNear(true);observer.disconnect();} },{rootMargin:"900px 0px"});
    observer.observe(video.current!);
    return ()=>observer.disconnect();
  },[]);
  useEffect(() => {
    const el = video.current!;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    const saveData = (navigator as Navigator & {connection?: {saveData?:boolean}}).connection?.saveData;
    let visible = false, manual: boolean | undefined = requested || undefined, automaticPause = false;
    const path = filmFile(id);
    function pause() {
      if (el.paused) return;
      automaticPause=true; el.pause();
    }
    function play() {
      if (!el.getAttribute("src")) el.src=path;
      void el.play().catch(()=>{});
    }
    function sync() {
      if (!visible || document.hidden || !(manual ?? (!reduced.matches && !saveData))) pause();
      else play();
    }
    const onPlay = () => {setPlaying(true);};
    const onPause = () => {setPlaying(false);if(!automaticPause && visible) manual=false;automaticPause=false;};
    toggle.current = () => {manual = el.paused; if(manual) play(); else pause();};
    const observer=new IntersectionObserver(([entry])=>{
      visible=entry.isIntersecting && entry.intersectionRatio>=.2;
      if (visible && controls && !el.getAttribute("src")) el.src=path;
      sync();
    },{threshold:[0,.2]});
    el.muted=true; observer.observe(el);
    el.addEventListener("play",onPlay);el.addEventListener("pause",onPause);
    reduced.addEventListener("change",sync);document.addEventListener("visibilitychange",sync);
    return ()=>{
      observer.disconnect();reduced.removeEventListener("change",sync);document.removeEventListener("visibilitychange",sync);
      el.removeEventListener("play",onPlay);el.removeEventListener("pause",onPause);el.pause();
    };
  },[id,controls,requested]);
  return <>
    <video ref={video} className="inline-film" poster={near ? filmFile(id,"jpg") : undefined} preload="none" muted loop playsInline controls={controls} aria-label={title}/>
    {!controls && <button type="button" className="inline-transport" aria-label={`${playing?"Pause":"Play"} ${title} preview`}
      onClick={()=>toggle.current()}><PlaybackMark playing={playing}/></button>}
  </>;
}
