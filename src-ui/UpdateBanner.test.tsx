import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { UpdateBanner } from "./UpdateBanner";
import type { UpdaterState } from "./useUpdater";

afterEach(cleanup);

function state(overrides: Partial<UpdaterState> = {}): UpdaterState {
  return {
    available: true,
    version: "1.2.3",
    body: "Notes",
    date: null,
    downloading: false,
    progress: 0,
    error: null,
    dismissed: false,
    checking: false,
    hasChecked: true,
    dialogOpen: false,
    ...overrides,
  };
}

test("renders nothing when no update is available or dismissed", () => {
  const { container, rerender } = render(
    <UpdateBanner
      state={state({ available: false })}
      channel="stable"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={() => {}}
    />,
  );
  expect(container).toBeEmptyDOMElement();

  rerender(
    <UpdateBanner
      state={state({ dismissed: true })}
      channel="stable"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={() => {}}
    />,
  );
  expect(container).toBeEmptyDOMElement();
});

test("shows the available state and fires install/dismiss", () => {
  const onInstall = vi.fn();
  const onDismiss = vi.fn();
  render(
    <UpdateBanner
      state={state()}
      channel="stable"
      onInstall={onInstall}
      onDismiss={onDismiss}
      onRetry={() => {}}
    />,
  );
  expect(screen.getByText("Burnrate 1.2.3 is available")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Install & Restart"));
  fireEvent.click(screen.getByTitle("Dismiss"));
  expect(onInstall).toHaveBeenCalledOnce();
  expect(onDismiss).toHaveBeenCalledOnce();
});

test("omits the body line when there are no release notes", () => {
  render(
    <UpdateBanner
      state={state({ body: null })}
      channel="stable"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={() => {}}
    />,
  );
  expect(screen.getByText("Burnrate 1.2.3 is available")).toBeInTheDocument();
  expect(screen.queryByText("Notes")).not.toBeInTheDocument();
});

test("labels the nightly channel distinctly", () => {
  render(
    <UpdateBanner
      state={state()}
      channel="nightly"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={() => {}}
    />,
  );
  expect(
    screen.getByText("Burnrate Nightly 1.2.3 is available"),
  ).toBeInTheDocument();
});

test("renders the downloading progress bar", () => {
  render(
    <UpdateBanner
      state={state({ downloading: true, progress: 42 })}
      channel="stable"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={() => {}}
    />,
  );
  expect(screen.getByText("Downloading update…")).toBeInTheDocument();
  expect(screen.getByText("42%")).toBeInTheDocument();
});

test("renders the error state with retry", () => {
  const onRetry = vi.fn();
  render(
    <UpdateBanner
      state={state({ error: "boom" })}
      channel="stable"
      onInstall={() => {}}
      onDismiss={() => {}}
      onRetry={onRetry}
    />,
  );
  expect(screen.getByText("boom")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Try again"));
  expect(onRetry).toHaveBeenCalledOnce();
});
