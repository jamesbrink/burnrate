import { afterEach, describe, expect, it } from "vitest";
import { installContextMenuGuard, isEditableTarget } from "./nativeChrome";

function contextMenuEvent(): MouseEvent {
  return new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("isEditableTarget", () => {
  it("treats inputs, textareas, and contenteditable as editable", () => {
    document.body.innerHTML = `
      <input id="field" />
      <textarea id="area"></textarea>
      <div id="rich" contenteditable="true"><span id="inner">x</span></div>
      <div id="bare" contenteditable></div>
      <div id="plaintext" contenteditable="plaintext-only"></div>
      <div id="optout" contenteditable="false"></div>
      <div id="plain">text</div>
    `;
    expect(isEditableTarget(document.getElementById("field"))).toBe(true);
    expect(isEditableTarget(document.getElementById("area"))).toBe(true);
    expect(isEditableTarget(document.getElementById("rich"))).toBe(true);
    // Nested inside a contenteditable region still counts.
    expect(isEditableTarget(document.getElementById("inner"))).toBe(true);
    // Bare boolean and plaintext-only forms are editable; "false" is not.
    expect(isEditableTarget(document.getElementById("bare"))).toBe(true);
    expect(isEditableTarget(document.getElementById("plaintext"))).toBe(true);
    expect(isEditableTarget(document.getElementById("optout"))).toBe(false);
    expect(isEditableTarget(document.getElementById("plain"))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe("installContextMenuGuard", () => {
  it("blocks the menu everywhere except editable elements", () => {
    const uninstall = installContextMenuGuard(document);
    document.body.innerHTML = `<div id="plain">text</div><input id="field" />`;

    const blocked = contextMenuEvent();
    document.getElementById("plain")!.dispatchEvent(blocked);
    expect(blocked.defaultPrevented).toBe(true);

    const allowed = contextMenuEvent();
    document.getElementById("field")!.dispatchEvent(allowed);
    expect(allowed.defaultPrevented).toBe(false);

    uninstall();
  });

  it("stops blocking after uninstall", () => {
    const uninstall = installContextMenuGuard(document);
    document.body.innerHTML = `<div id="plain">text</div>`;
    uninstall();

    const event = contextMenuEvent();
    document.getElementById("plain")!.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});
