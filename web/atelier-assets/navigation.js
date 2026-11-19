(() => {
  // Native print strokes can be less than a tenth of a screen pixel at thumbnail size.
  // Keep the exported path geometry and give the display copy its own ink and paper.
  window.workbenchSheetPreview = svg => {
    const ink = '<style>svg>rect{fill:#f7f2e7}.cut path,.cut-exterior{fill:#d9a782;stroke:#6c3f2d;stroke-width:1px;vector-effect:non-scaling-stroke}g[data-piece]:nth-of-type(even) .cut path,g[data-instance]:nth-of-type(even) .cut-exterior{fill:#93a99b;stroke:#394d42}.sew,.notches,.grain,.annotations,.labels,text{display:none}</style>';
    return URL.createObjectURL(new Blob([svg.replace(/<\/svg>\s*$/, ink + "</svg>")], {type:"image/svg+xml"}));
  };
  const pages = [
    ["pattern", "index.html", "Patterns", '<path d="m4 4 7-2 4 5 5 2-4 12-12-3z"/><path d="m6 7 4-2m-3 5 6 2m-7 2 4 1"/>'],
    ["materials", "materials.html", "Materials", '<path d="m3 9 9-5 9 5-9 5zM3 13l9 5 9-5M3 17l9 5 9-5"/>'],
    ["photo", "photo.html", "Photo measure", '<rect x="2" y="6" width="20" height="15" rx="2"/><path d="m6 6 2-3h7l2 3"/><circle cx="12" cy="13.5" r="4"/>'],
    ["trace", "trace.html", "Tracing", '<path d="m3 18 5-13 10 3 3 10-12 2z" stroke-dasharray="3 3"/><circle cx="8" cy="5" r="1.5"/><circle cx="21" cy="18" r="1.5"/>'],
  ];
  const icon = content => '<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.6">' + content + '</svg>';
  const mast = document.createElement("header");
  mast.className = "wb-mast";
  const inGallery = location.pathname.includes("/workbench/");
  mast.innerHTML = '<div class="wb-mast-inner"><a class="wb-brand" href="' + (inGallery ? "../" : "index.html") + '">' +
    // The same four slanted bars as the site header, so the workbench reads as part of it.
    '<svg aria-hidden="true" viewBox="0 0 34 28"><g transform="translate(9.5 0) skewX(-19)" fill="currentColor"><rect x="1" y="5" width="4.6" height="17" rx=".8"/><rect x="7.5" width="4.6" height="26" rx=".8"/><rect x="14" y="5" width="4.6" height="22" rx=".8"/><rect x="20.5" y="8" width="4.6" height="14" rx=".8"/></g></svg>' +
    'Franken<span>Textiles</span></a><nav aria-label="Workbenches">' +
    (inGallery ? '<a class="wb-back" href="../">' + icon('<path d="M19 12H5m6-6-6 6 6 6"/>') + 'Exhibition</a>' : '') +
    pages.map(([name, path, title, design]) => '<a href="' + path + '"' + (document.body.classList.contains("wb-" + name) ? ' aria-current="page"' : '') + '>' + icon(design) + title + '</a>').join("") +
    '</nav><button type="button" class="wb-theme" aria-label="Change color theme">' +
    icon('<path d="M20.5 14A8.7 8.7 0 0 1 10 3.5 8.9 8.9 0 1 0 20.5 14Z"/>') + '</button></div>';
  document.body.prepend(mast);
  const system = matchMedia("(prefers-color-scheme: dark)");
  const toggle = mast.querySelector("button");
  let saved;
  try { saved = localStorage.getItem("ftx-workbench-theme"); } catch {}
  function theme() {
    const dark = saved ? saved === "dark" : system.matches;
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    toggle.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
    toggle.title = toggle.getAttribute("aria-label");
  }
  toggle.addEventListener("click", () => {
    saved = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    try { localStorage.setItem("ftx-workbench-theme", saved); } catch {}
    theme();
  });
  system.addEventListener("change", theme);
  theme();

  function fold(section) {
    const heading = section?.querySelector(":scope > h2");
    if (!heading) return;
    const details = document.createElement("details"), summary = document.createElement("summary");
    details.className = "wb-fold"; summary.append(heading);
    details.append(summary, ...section.childNodes); section.append(details);
    section.classList.add("wb-fold-panel");
  }
  if (document.body.classList.contains("wb-materials")) {
    for (const section of document.querySelectorAll("#materialConsumers, #materialComparison, [aria-labelledby='studySaveHeading']")) fold(section);
  }
  if (document.body.classList.contains("wb-photo")) {
    const sections = [...document.querySelectorAll("main > section")];
    for (const section of sections.filter(s => /^[456]\./.test(s.querySelector(":scope > h2")?.textContent))) fold(section);
    const reference = sections.find(s => s.querySelector(":scope > h2")?.textContent.startsWith("1."));
    if (reference) reference.after(sections[0]);
  }
  if (document.body.classList.contains("wb-trace")) {
    document.getElementById("edits").before(document.getElementById("results"));
    fold(document.getElementById("edits"));
  }
  function revealFragment() {
    if (!location.hash) return;
    let node = document.getElementById(location.hash.slice(1));
    const target = node;
    if (node?.matches(".wb-fold-panel")) node.querySelector(":scope > details").open = true;
    while (node) {
      if (node instanceof HTMLDetailsElement) node.open = true;
      node = node.parentElement;
    }
    target?.scrollIntoView({block:"start"});
  }
  window.addEventListener("hashchange", revealFragment);
  revealFragment();
  const footer = document.createElement("footer");
  footer.className = "wb-footer";
  footer.innerHTML = '<b>FrankenTextiles</b><span>Files and calculations stay in your browser.</span>' +
    (inGallery ? '<nav aria-label="FrankenTextiles site"><a href="../">Exhibition</a><a href="../how-it-works/">How it works</a>' +
      '<a href="../get-started/">Get started</a><a href="https://github.com/Dicklesworthstone/franken_textiles">Source</a></nav>' : '');
  document.body.append(footer);
})();
