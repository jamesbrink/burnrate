const DEFAULT_LINUX_UI_SCALE = 1.35;
const MIN_UI_SCALE = 1;
const MAX_UI_SCALE = 2;
const MAX_REASONABLE_VIEWPORT_PX = 100_000;

let listenersInstalled = false;
let resizeFrame = 0;

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isLinuxRuntime(
  platform = navigator.platform,
  userAgent = navigator.userAgent,
): boolean {
  return /linux/i.test(`${platform} ${userAgent}`);
}

export function parseUiScale(value: unknown, fallback = 1): number {
  const scale =
    typeof value === "number"
      ? value
      : typeof value === "string"
        ? Number.parseFloat(value)
        : Number.NaN;
  if (!Number.isFinite(scale)) {
    return fallback;
  }
  return Math.max(MIN_UI_SCALE, Math.min(MAX_UI_SCALE, scale));
}

export function defaultUiScale(): number {
  if (hasTauri() && isLinuxRuntime()) {
    return DEFAULT_LINUX_UI_SCALE;
  }
  return 1;
}

function saneViewportSize(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value > 0 &&
    value < MAX_REASONABLE_VIEWPORT_PX
  );
}

function viewportSize(axis: "width" | "height"): number {
  const inner = axis === "width" ? window.innerWidth : window.innerHeight;
  const visual = window.visualViewport?.[axis];
  const outer = axis === "width" ? window.outerWidth : window.outerHeight;
  const docClient =
    axis === "width"
      ? document.documentElement.clientWidth
      : document.documentElement.clientHeight;
  const bodyClient =
    axis === "width" ? document.body?.clientWidth : document.body?.clientHeight;
  const screenAvail =
    axis === "width" ? window.screen?.availWidth : window.screen?.availHeight;
  const screenSize =
    axis === "width" ? window.screen?.width : window.screen?.height;

  for (const value of [
    inner,
    visual,
    outer,
    docClient,
    bodyClient,
    screenAvail,
    screenSize,
  ]) {
    if (saneViewportSize(value)) {
      return value;
    }
  }

  return axis === "width" ? 1024 : 768;
}

export function readUiScale(): number {
  const root = document.documentElement;
  return parseUiScale(
    root.style.getPropertyValue("--burnrate-ui-scale") || root.style.zoom,
    1,
  );
}

function writeViewportVars(scale: number): void {
  const root = document.documentElement;
  const safeScale = parseUiScale(scale, 1);
  root.style.setProperty(
    "--burnrate-viewport-width",
    `${viewportSize("width") / safeScale}px`,
  );
  root.style.setProperty(
    "--burnrate-viewport-height",
    `${viewportSize("height") / safeScale}px`,
  );
}

export function applyUiScale(scale = defaultUiScale()): void {
  const safeScale = parseUiScale(scale, 1);
  const root = document.documentElement;
  root.style.setProperty("--burnrate-ui-scale", String(safeScale));
  writeViewportVars(safeScale);
  root.style.zoom = String(safeScale);
}

function scheduleViewportWrite(): void {
  cancelAnimationFrame(resizeFrame);
  resizeFrame = requestAnimationFrame(() => writeViewportVars(readUiScale()));
}

export function installViewportScale(): void {
  applyUiScale();
  if (listenersInstalled) {
    return;
  }
  listenersInstalled = true;
  window.addEventListener("resize", scheduleViewportWrite);
  window.visualViewport?.addEventListener("resize", scheduleViewportWrite);
}
