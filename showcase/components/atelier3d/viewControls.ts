import * as THREE from "three";
import type { OrbitControls } from "three/addons/controls/OrbitControls.js";
import "./view-controls.css";

// One camera grammar for every 3D view on the site.
//
//   mouse     drag turns · right-drag or Shift-drag moves · wheel zooms towards the pointer
//   touch     one finger turns · two fingers zoom and move
//   keyboard  arrows turn · Shift-arrows move · + and − zoom · Home or 0 resets
//   anywhere  double-click or the reset button returns to the opening view
//
// A view passes the OrbitControls it already owns; this adds what each one used to leave out and
// draws the same three buttons in its corner. A view keeps whatever framing it needs through
// `home`, and repaints through `paint` when it renders on demand.

type Camera = THREE.OrthographicCamera | THREE.PerspectiveCamera;

export interface ViewOptions {
  /** Repaint after a camera change (views that render on demand). */
  paint?: () => void;
  /** The view's own opening framing. Without it the state saved by `setHome` is restored. */
  home?: () => void;
  /** How far the view may be moved from its opening target, as a fraction of the opening frame. */
  reach?: number;
  /** Where the buttons sit (half-way up the right edge by default). `false` draws none. */
  tools?: "right" | "bottom-right" | "bottom-left" | "top-right" | "top-left" | false;
  /** Arrow keys turn the view. Off for views that already bind them to something else. */
  turnKeys?: boolean;
  /** A dark stage: the buttons take the light-on-dark colours. */
  dark?: boolean;
}

export interface ViewControls {
  /** Return to the opening view. */
  home(): void;
  /** Take the present camera as the opening view (call after the view has framed its subject). */
  setHome(): void;
  /** The subject has moved: measure how far the view has been carried from this point from now on. */
  retarget(point: THREE.Vector3): void;
  /** Zoom by a factor (above one comes closer). */
  zoom(factor: number): void;
  dispose(): void;
}

// A wheel that is scrolling the page must not be captured by a view that slides under the pointer.
// A wheel that starts while the pointer rests on a view zooms it. The two are told apart by time:
// a view takes the wheel only if the page has not been scrolling for PAUSE_MS.
const PAUSE_MS = 420;
let pageWheelAt = -1e9;
let viewWheelAt = -1e9;
let listening = false;
let hinted = false;
const managed = new WeakSet<EventTarget>();

function watchPageWheel() {
  if (listening || typeof window === "undefined") return;
  listening = true;
  window.addEventListener("wheel", (event) => {
    if (!event.target || !managed.has(event.target)) pageWheelAt = event.timeStamp;
  }, { capture: true, passive: true });
}

function icon(path: string) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("aria-hidden", "true");
  const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
  line.setAttribute("d", path);
  svg.appendChild(line);
  return svg;
}

function button(label: string, path: string, act: () => void) {
  const element = document.createElement("button");
  element.type = "button";
  element.setAttribute("aria-label", label);
  element.title = label;
  element.appendChild(icon(path));
  element.addEventListener("click", act);
  return element;
}

export function fullControls(orbit: OrbitControls, canvas: HTMLCanvasElement, options: ViewOptions = {}): ViewControls {
  const camera = orbit.object as Camera;
  const repaint = () => options.paint?.();
  watchPageWheel();
  managed.add(canvas);

  orbit.enableRotate = true;
  orbit.enableZoom = true;
  orbit.enablePan = true;
  orbit.screenSpacePanning = true;
  orbit.zoomToCursor = true;
  orbit.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.DOLLY, RIGHT: THREE.MOUSE.PAN };
  orbit.touches = { ONE: THREE.TOUCH.ROTATE, TWO: THREE.TOUCH.DOLLY_PAN };
  // A vertical swipe still scrolls the page; a swipe that starts sideways turns the view, and from
  // then on belongs to the view in both directions. OrbitControls would otherwise claim every touch.
  canvas.style.touchAction = "pan-y";
  if (!canvas.hasAttribute("tabindex")) canvas.tabIndex = 0;
  canvas.style.cursor = "grab";

  // The opening view, and how far the subject may be carried from it.
  const homeTarget = orbit.target.clone();
  let homeDistance = 1;
  function measure() {
    homeTarget.copy(orbit.target);
    homeDistance = camera.position.distanceTo(orbit.target);
  }
  // Half the height of the unzoomed frame, in world units. An orthographic view may reframe itself
  // as its subject changes, so its frame is read when needed and not remembered.
  function span() {
    const half = camera instanceof THREE.OrthographicCamera
      ? Math.min(camera.top - camera.bottom, camera.right - camera.left) / 2
      : homeDistance * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2);
    return half > 0 && Number.isFinite(half) ? half : 1;
  }
  measure();
  orbit.saveState();
  const offset = new THREE.Vector3();
  const keepInReach = () => {
    const limit = span() * (options.reach ?? 0.9);
    offset.copy(orbit.target).sub(homeTarget);
    const length = offset.length();
    if (length <= limit) return;
    // Carry the camera back with the target so the direction of view is kept.
    offset.multiplyScalar(limit / length - 1);
    orbit.target.add(offset);
    camera.position.add(offset);
    repaint();                              // listeners added before this one painted the unclamped view
  };

  function home() {
    if (options.home) options.home();
    else orbit.reset();
    repaint();
  }
  function zoom(factor: number) {
    if (camera instanceof THREE.OrthographicCamera) {
      camera.zoom = THREE.MathUtils.clamp(camera.zoom * factor, orbit.minZoom, orbit.maxZoom);
      camera.updateProjectionMatrix();
    } else {
      offset.copy(camera.position).sub(orbit.target);
      const distance = THREE.MathUtils.clamp(offset.length() / factor, orbit.minDistance, orbit.maxDistance);
      camera.position.copy(orbit.target).add(offset.setLength(distance));
    }
    orbit.update();
    repaint();
  }
  function move(right: number, up: number) {
    const step = span() / (camera instanceof THREE.OrthographicCamera ? camera.zoom : 1) * 0.12;
    offset.setFromMatrixColumn(camera.matrix, 0).multiplyScalar(right * step);
    orbit.target.add(offset);
    camera.position.add(offset);
    offset.setFromMatrixColumn(camera.matrix, 1).multiplyScalar(up * step);
    orbit.target.add(offset);
    camera.position.add(offset);
    orbit.update();
    repaint();
  }
  function turn(azimuth: number, polar: number) {
    const spherical = new THREE.Spherical().setFromVector3(offset.copy(camera.position).sub(orbit.target));
    spherical.theta += azimuth;
    spherical.phi = THREE.MathUtils.clamp(spherical.phi + polar, Math.max(orbit.minPolarAngle, 0.02), Math.min(orbit.maxPolarAngle, Math.PI - 0.02));
    camera.position.copy(orbit.target).add(offset.setFromSpherical(spherical));
    camera.lookAt(orbit.target);
    orbit.update();
    repaint();
  }

  const wheel = (event: WheelEvent) => {
    const now = event.timeStamp;
    const pageIsScrolling = now - pageWheelAt < PAUSE_MS && pageWheelAt > viewWheelAt;
    if (event.ctrlKey || event.metaKey || !pageIsScrolling) {
      viewWheelAt = now;
      return;                               // OrbitControls zooms and stops the page
    }
    pageWheelAt = now;
    event.stopImmediatePropagation();       // the page keeps scrolling
  };
  const keydown = (event: KeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const arrow = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[event.key];
    if (arrow && event.shiftKey) move(arrow[0], arrow[1]);
    else if (arrow && options.turnKeys) turn(-arrow[0] * Math.PI / 18, -arrow[1] * Math.PI / 18);
    else if (options.turnKeys && (event.key === "+" || event.key === "=")) zoom(1.2);
    else if (options.turnKeys && (event.key === "-" || event.key === "_")) zoom(1 / 1.2);
    else if (event.key === "0" || (options.turnKeys && event.key === "Home")) home();
    else return;
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const grabbing = () => { canvas.style.cursor = "grabbing"; };
  const released = () => { canvas.style.cursor = "grab"; };
  const menu = (event: Event) => event.preventDefault();   // the right button moves the view
  canvas.addEventListener("wheel", wheel, { capture: true, passive: true });
  canvas.addEventListener("keydown", keydown, { capture: true });
  canvas.addEventListener("dblclick", home);
  canvas.addEventListener("contextmenu", menu);
  orbit.addEventListener("change", keepInReach);
  orbit.addEventListener("start", grabbing);
  orbit.addEventListener("end", released);

  // The buttons, and a line of help the first time a pointer comes over any view on the page.
  let tools: HTMLDivElement | undefined;
  let hint: HTMLParagraphElement | undefined;
  let hintTimer = 0;
  const host = canvas.parentElement;
  const showHint = () => {
    if (hinted || !tools) return;
    hinted = true;
    hint = document.createElement("p");
    hint.className = "view-hint";
    hint.textContent = window.matchMedia("(pointer: coarse)").matches
      ? "Drag to turn · pinch to zoom · two fingers to move"
      : "Drag to turn · scroll to zoom · right-drag to move";
    tools.appendChild(hint);
    hintTimer = window.setTimeout(() => hint?.classList.add("is-gone"), 5200);
  };
  if (options.tools !== false && host) {
    if (getComputedStyle(host).position === "static") host.style.position = "relative";
    tools = document.createElement("div");
    tools.className = `view-tools view-tools-${options.tools ?? "right"}${options.dark ? " view-tools-dark" : ""}`;
    tools.setAttribute("role", "group");
    tools.setAttribute("aria-label", "View");
    tools.append(
      button("Zoom in", "M8 3.5v9M3.5 8h9", () => zoom(1.25)),
      button("Zoom out", "M3.5 8h9", () => zoom(1 / 1.25)),
      button("Reset the view", "M3.2 8a4.8 4.8 0 1 0 1.5-3.5M3 2.6v2.4h2.4", home),
    );
    host.appendChild(tools);
    canvas.addEventListener("pointerenter", showHint);
  }

  return {
    home,
    zoom,
    setHome() { measure(); orbit.saveState(); },
    retarget(point) { homeTarget.copy(point); },
    dispose() {
      window.clearTimeout(hintTimer);
      canvas.removeEventListener("wheel", wheel, { capture: true });
      canvas.removeEventListener("keydown", keydown, { capture: true });
      canvas.removeEventListener("dblclick", home);
      canvas.removeEventListener("contextmenu", menu);
      canvas.removeEventListener("pointerenter", showHint);
      orbit.removeEventListener("change", keepInReach);
      orbit.removeEventListener("start", grabbing);
      orbit.removeEventListener("end", released);
      managed.delete(canvas);
      tools?.remove();
    },
  };
}
