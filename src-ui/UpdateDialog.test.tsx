import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { UpdateDialog } from "./UpdateDialog";
import type { UpdaterState } from "./useUpdater";

afterEach(cleanup);

function state(overrides: Partial<UpdaterState> = {}): UpdaterState {
  return {
    available: false,
    version: null,
    body: null,
    date: null,
    downloading: false,
    progress: 0,
    error: null,
    dismissed: false,
    checking: false,
    hasChecked: true,
    dialogOpen: true,
    ...overrides,
  };
}

function renderDialog(
  s: UpdaterState,
  handlers: Partial<{
    onInstall: () => void;
    onRetry: () => void;
    onClose: () => void;
  }> = {},
) {
  return render(
    <UpdateDialog
      state={s}
      channel="stable"
      appVersion="1.0.0"
      onInstall={handlers.onInstall ?? (() => {})}
      onRetry={handlers.onRetry ?? (() => {})}
      onClose={handlers.onClose ?? (() => {})}
    />,
  );
}

test("renders nothing while the dialog is closed", () => {
  const { container } = renderDialog(state({ dialogOpen: false }));
  expect(container).toBeEmptyDOMElement();
});

test("shows the checking phase while a check is in flight", () => {
  renderDialog(state({ checking: true }));
  expect(screen.getByText("Checking for updates…")).toBeInTheDocument();
});

test("shows the error phase with retry and close", () => {
  const onRetry = vi.fn();
  const onClose = vi.fn();
  renderDialog(state({ error: "offline" }), { onRetry, onClose });

  expect(screen.getByText("Update check failed")).toBeInTheDocument();
  expect(screen.getByText("offline")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Try again"));
  fireEvent.click(screen.getByText("Close"));
  expect(onRetry).toHaveBeenCalledOnce();
  expect(onClose).toHaveBeenCalledOnce();
});

test("shows the available phase with notes, install, and later", () => {
  const onInstall = vi.fn();
  const onClose = vi.fn();
  renderDialog(
    state({
      available: true,
      version: "2.0.0",
      body: "- New things",
      date: "2026-06-01T00:00:00Z",
    }),
    { onInstall, onClose },
  );

  expect(screen.getByText("Burnrate 2.0.0 is available")).toBeInTheDocument();
  expect(screen.getByText(/You have v1\.0\.0\. Released/)).toBeInTheDocument();
  expect(screen.getByText("- New things")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Install & Restart"));
  fireEvent.click(screen.getByText("Later"));
  expect(onInstall).toHaveBeenCalledOnce();
  expect(onClose).toHaveBeenCalledOnce();
});

test("labels the nightly channel and skips an unparseable date", () => {
  render(
    <UpdateDialog
      state={state({ available: true, version: "2.0.0", date: "not a date" })}
      channel="nightly"
      appVersion="1.0.0"
      onInstall={() => {}}
      onRetry={() => {}}
      onClose={() => {}}
    />,
  );
  expect(
    screen.getByText("Burnrate Nightly 2.0.0 is available"),
  ).toBeInTheDocument();
  expect(screen.getByText("You have v1.0.0.")).toBeInTheDocument();
});

test("disables actions and blocks overlay close while downloading", () => {
  const onClose = vi.fn();
  renderDialog(
    state({
      available: true,
      version: "2.0.0",
      downloading: true,
      progress: 42,
    }),
    { onClose },
  );

  const install = screen.getByText("Installing… 42%");
  expect(install).toBeDisabled();
  expect(screen.getByText("Later")).toBeDisabled();
  fireEvent.click(screen.getByRole("presentation"));
  expect(onClose).not.toHaveBeenCalled();
});

test("shows the up-to-date phase and closes via OK and overlay", () => {
  const onClose = vi.fn();
  renderDialog(state(), { onClose });

  expect(screen.getByText("You’re up to date")).toBeInTheDocument();
  expect(
    screen.getByText(/Burnrate v1\.0\.0 is the latest version on the stable/),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByText("OK"));
  fireEvent.click(screen.getByRole("presentation"));
  expect(onClose).toHaveBeenCalledTimes(2);
});

test("explains unsupported builds when no check ever completed", () => {
  renderDialog(state({ hasChecked: false }));
  expect(
    screen.getByText("Updates aren’t available in this build"),
  ).toBeInTheDocument();
});
