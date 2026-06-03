import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const api = vi.hoisted(() => ({
  checkForUpdates: vi.fn(),
  installUpdate: vi.fn(),
  onCheckUpdateRequested: vi.fn(),
  onUpdateProgress: vi.fn(),
  updaterAvailable: vi.fn(),
}));

vi.mock("./api", () => api);

import { useUpdater } from "./useUpdater";

// Captured event handlers so tests can drive the progress / tray-check paths.
let progressHandler: ((pct: number) => void) | null = null;
let checkHandler: (() => void) | null = null;

beforeEach(() => {
  vi.clearAllMocks();
  progressHandler = null;
  checkHandler = null;
  api.updaterAvailable.mockResolvedValue(true);
  api.checkForUpdates.mockResolvedValue(null);
  api.installUpdate.mockResolvedValue(undefined);
  api.onUpdateProgress.mockImplementation((handler: (pct: number) => void) => {
    progressHandler = handler;
    return Promise.resolve(() => {});
  });
  api.onCheckUpdateRequested.mockImplementation((handler: () => void) => {
    checkHandler = handler;
    return Promise.resolve(() => {});
  });
});

afterEach(() => vi.restoreAllMocks());

test("background poll surfaces an available update", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: "Notes",
    date: null,
  });

  const { result } = renderHook(() =>
    useUpdater("stable", { forcePoll: true }),
  );

  await waitFor(() => expect(result.current.state.available).toBe(true));
  expect(result.current.state.version).toBe("2.0.0");
  expect(api.checkForUpdates).toHaveBeenCalledWith("stable");
});

test("up-to-date check marks hasChecked without availability", async () => {
  const { result } = renderHook(() =>
    useUpdater("nightly", { forcePoll: true }),
  );

  await waitFor(() => expect(result.current.state.hasChecked).toBe(true));
  expect(result.current.state.available).toBe(false);
  expect(api.checkForUpdates).toHaveBeenCalledWith("nightly");
});

test("stays hidden when the updater is unavailable", async () => {
  api.updaterAvailable.mockResolvedValue(false);
  const { result } = renderHook(() =>
    useUpdater("stable", { forcePoll: true }),
  );

  await waitFor(() => expect(api.updaterAvailable).toHaveBeenCalled());
  expect(result.current.state.available).toBe(false);
  expect(result.current.state.hasChecked).toBe(false);
  expect(api.checkForUpdates).not.toHaveBeenCalled();
});

test("install streams progress and recovers from failure", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: null,
    date: null,
  });
  api.installUpdate.mockRejectedValueOnce(new Error("disk full"));

  const { result } = renderHook(() =>
    useUpdater("stable", { forcePoll: true }),
  );
  await waitFor(() => expect(result.current.state.available).toBe(true));

  // A progress event flows through to state.
  act(() => progressHandler?.(55));
  expect(result.current.state.progress).toBe(55);

  await act(async () => {
    await result.current.install();
  });
  expect(result.current.state.error).toContain("disk full");
  expect(result.current.state.downloading).toBe(false);
});

test("dismiss hides the current version", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: null,
    date: null,
  });
  const { result } = renderHook(() =>
    useUpdater("stable", { forcePoll: true }),
  );
  await waitFor(() => expect(result.current.state.available).toBe(true));

  act(() => result.current.dismiss());
  expect(result.current.state.dismissed).toBe(true);
});

test("re-checking the same dismissed version stays dismissed", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: null,
    date: null,
  });
  const { result } = renderHook(() => useUpdater("stable"));

  await act(async () => {
    await result.current.checkNow();
  });
  expect(result.current.state.available).toBe(true);

  act(() => result.current.dismiss());
  await act(async () => {
    await result.current.checkNow();
  });
  // Same version → still hidden.
  expect(result.current.state.dismissed).toBe(true);
});

test("install is a no-op before any update is found", async () => {
  const { result } = renderHook(() => useUpdater("stable"));
  await act(async () => {
    await result.current.install();
  });
  // No pending version → nothing to install, backend not called.
  expect(api.installUpdate).not.toHaveBeenCalled();
  expect(result.current.state.downloading).toBe(false);
});

test("install passes the displayed version to the backend", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: null,
    date: null,
  });
  api.installUpdate.mockResolvedValue(undefined);

  const { result } = renderHook(() => useUpdater("stable"));
  await act(async () => {
    await result.current.checkNow();
  });
  await act(async () => {
    await result.current.install();
  });
  expect(api.installUpdate).toHaveBeenCalledWith("2.0.0");
});

test("install is a no-op while already downloading", async () => {
  api.checkForUpdates.mockResolvedValue({
    version: "2.0.0",
    currentVersion: "1.0.0",
    body: null,
    date: null,
  });
  // Never resolves → keeps `downloading` true for the duration of the test.
  api.installUpdate.mockReturnValue(new Promise<void>(() => {}));

  const { result } = renderHook(() => useUpdater("stable"));
  await act(async () => {
    await result.current.checkNow();
  });

  act(() => void result.current.install());
  expect(result.current.state.downloading).toBe(true);
  act(() => void result.current.install());
  // The second call returns early without re-invoking the backend.
  expect(api.installUpdate).toHaveBeenCalledOnce();
});

test("concurrent checks are de-duplicated into one backend call", async () => {
  let resolve: (v: null) => void = () => {};
  api.checkForUpdates.mockReturnValue(
    new Promise<null>((r) => {
      resolve = r;
    }),
  );

  const { result } = renderHook(() => useUpdater("stable"));
  await act(async () => {
    const a = result.current.checkNow();
    const b = result.current.checkNow();
    resolve(null);
    await Promise.all([a, b]);
  });

  expect(api.checkForUpdates).toHaveBeenCalledOnce();
});

test("the tray menu check triggers a fresh check", async () => {
  renderHook(() => useUpdater("stable"));
  await waitFor(() => expect(checkHandler).toBeTypeOf("function"));

  api.checkForUpdates.mockResolvedValue(null);
  await act(async () => {
    checkHandler?.();
  });
  expect(api.checkForUpdates).toHaveBeenCalledWith("stable");
});
