"use client";

import { useEffect, useRef, useState } from "react";

const revisions: Record<string, string> = JSON.parse(process.env.NEXT_PUBLIC_FILM_REVISIONS ?? "{}");
export function filmFile(id: string, ext: "mp4" | "jpg" = "mp4") {
  const name = `${id}.${ext}`;
  return revisions[name] ? `/media/${name}?v=${revisions[name]}` : `/media/${name}`;
}

// A 3840×2160 film at 60 frames a second is H.264 High at level 5.2. Many phones and older laptops decode
// no further than 1080p60 or 2160p30, so every film also ships as <id>-1080.mp4 at 1920×1080 and 60 fps.
const UHD = {contentType:'video/mp4; codecs="avc1.640034"', width:3840, height:2160, bitrate:9_000_000, framerate:60};
const REMEMBERED = "ftx-film-1080";
let uhd: Promise<boolean> | undefined;
function playsUhd() {
  return uhd ??= (async () => {
    try { if (sessionStorage.getItem(REMEMBERED)) return false; } catch {}
    if ((navigator as Navigator & {connection?: {saveData?:boolean}}).connection?.saveData) return false;
    try {
      const info = await navigator.mediaCapabilities.decodingInfo({type:"file", video:UHD});
      // A phone that has to decode this in software drops frames and drains the battery.
      return info.supported && info.smooth && (info.powerEfficient || !matchMedia("(pointer: coarse)").matches);
    } catch { return true; } // No answer: try the 4K film. watchFilm steps down if it fails or stutters.
  })();
}
/** The film to play here: the 4K one where the device decodes it smoothly, otherwise the 1080p copy. */
export async function filmSource(id: string) {
  const smaller = id+"-1080";
  return filmFile(revisions[`${smaller}.mp4`] && !await playsUhd() ? smaller : id);
}
/** Step a playing 4K film down to its 1080p copy if it cannot be decoded, or if a third of its frames are dropped. */
export function watchFilm(el: HTMLVideoElement, id: string, resume = () => { void el.play().catch(()=>{}); }) {
  if (!revisions[`${id}-1080.mp4`]) return () => {};
  const small = filmFile(id+"-1080");
  let pending: number | undefined;
  let wasPlaying = !el.paused;
  function onPlay() { wasPlaying = true; }
  function onPause() { if (!el.error) wasPlaying = false; }
  function stepDown() {
    const source = el.getAttribute("src");
    if (!source || source === small || pending !== undefined) return;
    const at = el.currentTime;
    // Finish the failed media load before starting another. Chromium otherwise aborts the new play request.
    pending = window.setTimeout(() => {
      pending = undefined;
      if (el.getAttribute("src") !== source) return;
      uhd = Promise.resolve(false);
      try { sessionStorage.setItem(REMEMBERED, "1"); } catch {}
      el.src = small;
      if (at > 0) el.currentTime = at;
      if (wasPlaying) resume();
    });
  }
  const onProgress = () => {
    const quality = el.getVideoPlaybackQuality?.();
    if (quality && quality.totalVideoFrames >= 120 && quality.droppedVideoFrames > quality.totalVideoFrames/3) stepDown();
  };
  el.addEventListener("play", onPlay); el.addEventListener("pause", onPause);
  el.addEventListener("error", stepDown); el.addEventListener("timeupdate", onProgress);
  return () => {
    window.clearTimeout(pending);
    el.removeEventListener("play", onPlay); el.removeEventListener("pause", onPause);
    el.removeEventListener("error", stepDown); el.removeEventListener("timeupdate", onProgress);
  };
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
    let path: string | undefined, live = true;
    let pendingPlay: Promise<void> | undefined;
    let playingSource: string | null = null;
    void filmSource(id).then(source => { if (!live) return; path=source; arm(); sync(); });
    const unwatch = watchFilm(el, id, sync);
    function pause() {
      if (el.paused) return;
      automaticPause=true; el.pause();
    }
    // With native controls the film needs its source as soon as it is in view, before anything plays.
    function arm() {
      if (path && visible && controls && !el.getAttribute("src")) el.src=path;
    }
    function play() {
      if (!path || !el.paused) return;
      if (!el.getAttribute("src")) el.src=path;
      const request = pendingPlay = el.play();
      void request.catch(() => { if (pendingPlay === request) pendingPlay = undefined; });
    }
    function sync() {
      if (!visible || document.hidden || !(manual ?? (!reduced.matches && !saveData))) pause();
      else play();
    }
    const onPlay = () => {
      if (!pendingPlay) manual = true; // Native video controls start playback outside our play function.
      pendingPlay = undefined;
      playingSource = el.getAttribute("src");
      setPlaying(true);
    };
    const onPause = () => {
      if (!automaticPause && visible && !el.error && el.getAttribute("src") === playingSource) manual = false;
      automaticPause = false;
      setPlaying(false);
    };
    toggle.current = () => {manual = el.paused; if(manual) play(); else pause();};
    const observer=new IntersectionObserver(([entry])=>{
      visible=entry.isIntersecting && entry.intersectionRatio>=.2;
      arm();
      sync();
    },{threshold:[0,.2]});
    el.muted=true; observer.observe(el);
    el.addEventListener("play",onPlay);el.addEventListener("pause",onPause);
    reduced.addEventListener("change",sync);document.addEventListener("visibilitychange",sync);
    return ()=>{
      live=false;unwatch();
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
