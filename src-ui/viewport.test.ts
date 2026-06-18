import { afterEach, expect, test } from "vitest";
import {
  applyUiScale,
  isLinuxRuntime,
  parseUiScale,
  readUiScale,
} from "./viewport";

const originalInnerWidth = Object.getOwnPropertyDescriptor(
  window,
  "innerWidth",
);
const originalInnerHeight = Object.getOwnPropertyDescriptor(
  window,
  "innerHeight",
);

function setWindowSize(width: number, height: number) {
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    value: width,
  });
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: height,
  });
}

afterEach(() => {
  const root = document.documentElement;
  root.style.removeProperty("--burnrate-ui-scale");
  root.style.removeProperty("--burnrate-viewport-width");
  root.style.removeProperty("--burnrate-viewport-height");
  root.style.zoom = "";
  if (originalInnerWidth) {
    Object.defineProperty(window, "innerWidth", originalInnerWidth);
  }
  if (originalInnerHeight) {
    Object.defineProperty(window, "innerHeight", originalInnerHeight);
  }
});

test("detects Linux user agents", () => {
  expect(isLinuxRuntime("Linux x86_64", "Mozilla/5.0")).toBe(true);
  expect(isLinuxRuntime("MacIntel", "Mozilla/5.0 Mac OS X")).toBe(false);
});

test("parses and clamps UI scale", () => {
  expect(parseUiScale("1.4")).toBe(1.4);
  expect(parseUiScale("0.5")).toBe(1);
  expect(parseUiScale("3")).toBe(2);
  expect(parseUiScale("nope", 1.2)).toBe(1.2);
});

test("applies root zoom and compensated viewport variables", () => {
  setWindowSize(1350, 900);

  applyUiScale(1.35);

  expect(readUiScale()).toBe(1.35);
  expect(document.documentElement.style.zoom).toBe("1.35");
  expect(
    Number.parseFloat(
      document.documentElement.style.getPropertyValue(
        "--burnrate-viewport-width",
      ),
    ),
  ).toBeCloseTo(1000);
  expect(
    Number.parseFloat(
      document.documentElement.style.getPropertyValue(
        "--burnrate-viewport-height",
      ),
    ),
  ).toBeCloseTo(666.67);
});
