"use client";

import { useState } from "react";
import "./site.css";

// Lines starting with "#" are comments and lines starting with ">" are program output;
// neither is copied, so the clipboard holds only what can be pasted into a shell.
export default function CodeBlock({ title, code }: { title: string; code: string }) {
  const [copied, setCopied] = useState(false);
  const lines = code.trim().split("\n");
  const commands = lines.filter(line => line && !line.startsWith("#") && !line.startsWith(">")).join("\n");
  async function copy() {
    try {
      await navigator.clipboard.writeText(commands);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch { setCopied(false); }
  }
  return <figure className="ft-code">
    <figcaption className="mono"><span>{title}</span>
      <button type="button" className="ft-copy" onClick={copy} aria-live="polite">{copied ? "Copied" : "Copy"}</button></figcaption>
    <pre tabIndex={0}><code>{lines.map((line, index) => <span key={index} className={line.startsWith("#") ? "c" : line.startsWith(">") ? "o" : undefined}>
      {line.startsWith(">") ? line.slice(2) : line}{"\n"}</span>)}</code></pre>
  </figure>;
}
