"use client";

import { useEffect, useRef, useState } from "react";

export default function InlineFilm({ id, title, controls = false, requested = false }: { id: string; title: string; controls?: boolean; requested?: boolean }) {
  const video = useRef<HTMLVideoElement>(null);
  const toggle = useRef<() => void>(()=>{});
  const [playing,setPlaying] = useState(false);
  useEffect(() => {
    const el = video.current!;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    const saveData = (navigator as Navigator & {connection?: {saveData?:boolean}}).connection?.saveData;
    let visible = false, manual: boolean | undefined = requested || undefined, automaticPause = false;
    const path = "/media/" + id + ".mp4";
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
    <video ref={video} className="inline-film" poster={`/media/${id}.jpg`} preload="none" muted loop playsInline controls={controls} aria-label={title}/>
    {!controls && <button type="button" className="inline-transport" aria-label={`${playing?"Pause":"Play"} ${title} preview`}
      onClick={()=>toggle.current()}><span aria-hidden="true">{playing?"Ⅱ":"▶"}</span></button>}
  </>;
}
