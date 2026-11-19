"use client";

import { useEffect } from "react";
import { usePathname } from "next/navigation";

// Fades in elements marked data-reveal as they scroll into view. Everything already on screen
// is shown before the hidden state is switched on, so nothing visible ever blinks out.
export default function Reveal() {
  const pathname = usePathname();
  useEffect(() => {
    const root = document.documentElement;
    const pending = [...document.querySelectorAll<HTMLElement>("[data-reveal]:not(.in)")];
    if (!pending.length || matchMedia("(prefers-reduced-motion: reduce)").matches || !("IntersectionObserver" in window)) {
      for (const el of pending) el.classList.add("in");
      return;
    }
    for (const el of pending) if (el.getBoundingClientRect().top < innerHeight * .92) el.classList.add("in");
    root.classList.add("reveal-on");
    const observer = new IntersectionObserver(entries => {
      const shown = entries.filter(entry => entry.isIntersecting);
      shown.forEach((entry, index) => {
        const el = entry.target as HTMLElement;
        el.style.setProperty("--d", `${Math.min(index, 5) * 70}ms`);
        el.classList.add("in");
        observer.unobserve(el);
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: .08 });
    for (const el of pending) if (!el.classList.contains("in")) observer.observe(el);
    return () => observer.disconnect();
  }, [pathname]);
  return null;
}
