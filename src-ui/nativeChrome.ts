/**
 * Suppress the WebKit default context menu so the app feels native instead of
 * like a web page ("Reload", "Look Up", "Translate", …). Editable elements
 * keep the menu — right-click Paste matters for API-key and auth-code fields.
 *
 * Lives outside main.tsx so jsdom tests can exercise it directly.
 */

export function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest('input, textarea, [contenteditable="true"]') !== null
  );
}

/** Install the guard on `doc`; returns an uninstall closure. */
export function installContextMenuGuard(doc: Document): () => void {
  function onContextMenu(event: MouseEvent) {
    if (!isEditableTarget(event.target)) {
      event.preventDefault();
    }
  }
  doc.addEventListener("contextmenu", onContextMenu, { capture: true });
  return () =>
    doc.removeEventListener("contextmenu", onContextMenu, { capture: true });
}
