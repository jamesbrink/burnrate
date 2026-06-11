import {
  FormEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  closePreferences,
  detectAccounts,
  getAppVersion,
  guardedFetch,
  isStale,
  logoutAccount,
  markFetched,
  onDashboardUpdated,
  onRefreshRequested,
  onSettingsUpdated,
  openPreferences,
  readCachedDashboard,
  removeAccount,
  reorderAccounts,
  resizePreferencesToContent,
  resizeTrayToContent,
  saveAccount,
  saveSettings,
} from "./api";
import { useUpdater } from "./useUpdater";
import { LoginModal } from "./LoginModal";
import { UpdateDialog } from "./UpdateDialog";
import {
  OPENROUTER_DEFAULT_ENDPOINT,
  RUNPOD_DEFAULT_ENDPOINT,
  cloneDefaultAwsCategories,
  emptyForm,
  Preferences,
  providerLabels,
} from "./Preferences";
import { TrayPanel } from "./TrayPanel";
import type {
  AccountInput,
  AccountView,
  AppSettings,
  DashboardState,
  ProviderKind,
  UpdateChannel,
  UsageSnapshot,
} from "./types";
import { useLogin } from "./useLogin";

const TRAY_BASE_WIDTH = 360;
const TRAY_MIN_SCALE = 0.5;
const TRAY_MAX_SCALE = 1;

export function App() {
  const isTrayView =
    new URLSearchParams(window.location.search).get("view") === "tray";
  const [state, setState] = useState<DashboardState | null>(
    () => readCachedDashboard()?.dashboard ?? null,
  );
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>(
    () => readCachedDashboard()?.dashboard.snapshots ?? [],
  );
  const [form, setForm] = useState<AccountInput>(emptyForm);
  const [activeId, setActiveId] = useState<string | null>(null);
  // Spinner only on a true cold start (no cached data to show).
  const [busy, setBusy] = useState(() => readCachedDashboard() === null);
  const [error, setError] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState("");
  const lastPreferenceSize = useRef({ width: 0, height: 0 });
  const lastTraySize = useRef({ width: 0, height: 0 });

  const updateChannel: UpdateChannel =
    state?.settings?.updateChannel ?? "stable";
  // Only the Preferences window actively checks (poll + tray-menu entry); the
  // tray view passively mirrors the backend's update-available broadcast.
  const updater = useUpdater(updateChannel, { enabled: !isTrayView });

  useEffect(() => {
    void getAppVersion().then(setAppVersion);
  }, []);
  // Mirror of `state` so the mount-captured `revalidate` can decide whether to
  // show the cold-start spinner without going stale.
  const stateRef = useRef<DashboardState | null>(state);
  stateRef.current = state;

  const login = useLogin({
    onCompleted: async (account) => {
      setActiveId(account.id);
      setForm(emptyForm);
      try {
        const dashboard = await guardedFetch({ force: true });
        setState(dashboard);
        setSnapshots(dashboard.snapshots);
      } catch (err) {
        setError(String(err));
      }
    },
  });

  async function revalidate(options: { force?: boolean } = {}) {
    // Background refreshes (tray open, stale revalidation) shouldn't flash a
    // spinner when we already have data to show; cold starts and explicit
    // manual refreshes do.
    const showSpinner = options.force === true || stateRef.current === null;
    if (showSpinner) {
      setBusy(true);
    }
    setError(null);
    try {
      const dashboard = await guardedFetch(options);
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    } catch (err) {
      setError(String(err));
    } finally {
      if (showSpinner) {
        setBusy(false);
      }
    }
  }

  useEffect(() => {
    const cached = readCachedDashboard();
    if (cached && !isStale(cached.fetchedAt)) {
      return;
    }
    void revalidate();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (isTrayView) {
      return;
    }

    function onKeyDown(event: KeyboardEvent) {
      if (!event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key !== "w" && key !== "q") {
        return;
      }
      event.preventDefault();
      void closePreferences();
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isTrayView]);

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onRefreshRequested(() => void revalidate()).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        cleanup = unlisten;
      }
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);

  const accounts = state?.accounts ?? [];
  const summary = useMemo(
    () => summaryFromBackend(state?.traySummary, snapshots),
    [state?.traySummary, snapshots],
  );
  const settings: AppSettings = {
    hideFromDock: state?.settings?.hideFromDock ?? true,
    updateChannel: state?.settings?.updateChannel ?? "stable",
    trayScale: state?.settings?.trayScale ?? TRAY_MAX_SCALE,
  };

  async function updateSettings(next: AppSettings) {
    const previousSettings = settings;
    setState((previous) =>
      previous ? { ...previous, settings: next } : previous,
    );
    try {
      await saveSettings(next);
    } catch (err) {
      setState((previous) =>
        previous ? { ...previous, settings: previousSettings } : previous,
      );
      setError(String(err));
    }
  }

  async function onUpdateChannelChange(channel: UpdateChannel) {
    const previousSettings = settings;
    if (channel === previousSettings.updateChannel) {
      return;
    }
    // Optimistically reflect the choice; persist, then let the backend's
    // settings-updated event reconcile both windows. Roll back if the save
    // fails so the UI doesn't drift from the stored config.
    await updateSettings({ ...previousSettings, updateChannel: channel });
  }

  async function onTrayScaleChange(trayScale: number) {
    const nextScale = Math.max(
      TRAY_MIN_SCALE,
      Math.min(TRAY_MAX_SCALE, trayScale),
    );
    if (Math.abs(nextScale - settings.trayScale) < 0.001) {
      return;
    }
    await updateSettings({ ...settings, trayScale: nextScale });
  }

  useEffect(() => {
    let cleanupDashboard: (() => void) | undefined;
    let cleanupSettings: (() => void) | undefined;
    let disposed = false;
    void onDashboardUpdated((dashboard) => {
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
      // Backend push already carries fresh data — cache it and reset the
      // throttle window so we don't immediately re-fetch.
      markFetched(dashboard);
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        cleanupDashboard = unlisten;
      }
    });
    void onSettingsUpdated((settings) => {
      setState((previous) =>
        previous
          ? { ...previous, settings }
          : {
              accounts: [],
              snapshots: [],
              traySummary: summaryFromBackend(undefined, []),
              settings,
            },
      );
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        cleanupSettings = unlisten;
      }
    });
    return () => {
      disposed = true;
      cleanupDashboard?.();
      cleanupSettings?.();
    };
  }, []);

  useLayoutEffect(() => {
    if (isTrayView) {
      return;
    }
    const element = document.querySelector<HTMLElement>(".prefs-shell");
    if (!element) {
      return;
    }

    let frame = 0;
    const measure = () => {
      const px = (value: string) => Number.parseFloat(value) || 0;
      const style = window.getComputedStyle(element);
      const paddingX = px(style.paddingLeft) + px(style.paddingRight);
      const paddingY = px(style.paddingTop) + px(style.paddingBottom);
      const header = element.querySelector<HTMLElement>(".prefs-header");
      const notice = element.querySelector<HTMLElement>(".notice");
      const layout = element.querySelector<HTMLElement>(".prefs-layout");
      const sidebar = element.querySelector<HTMLElement>(".prefs-list");
      const main = element.querySelector<HTMLElement>(".prefs-main");
      const layoutStyle = layout ? window.getComputedStyle(layout) : null;
      const rowGap = layoutStyle
        ? px(layoutStyle.rowGap || layoutStyle.gap)
        : 0;
      const columnGap = layoutStyle
        ? px(layoutStyle.columnGap || layoutStyle.gap)
        : 0;
      const headerStyle = header ? window.getComputedStyle(header) : null;
      const headerMargin = headerStyle ? px(headerStyle.marginBottom) : 0;
      const noticeStyle = notice ? window.getComputedStyle(notice) : null;
      const noticeMargin = noticeStyle ? px(noticeStyle.marginBottom) : 0;
      const hasSingleColumn =
        layout !== null &&
        window.getComputedStyle(layout).gridTemplateColumns.split(" ").length <=
          1;
      const sidebarWidth = sidebar?.scrollWidth ?? 0;
      const mainWidth = main?.scrollWidth ?? 0;
      const layoutWidth = hasSingleColumn
        ? Math.max(sidebarWidth, mainWidth)
        : sidebarWidth + columnGap + mainWidth;
      const layoutHeight = hasSingleColumn
        ? (sidebar?.scrollHeight ?? 0) + rowGap + (main?.scrollHeight ?? 0)
        : Math.max(sidebar?.scrollHeight ?? 0, main?.scrollHeight ?? 0);
      const width = Math.ceil(
        Math.max(element.scrollWidth, layoutWidth + paddingX),
      );
      const height = Math.ceil(
        paddingY +
          (header?.offsetHeight ?? 0) +
          headerMargin +
          (notice?.offsetHeight ?? 0) +
          noticeMargin +
          layoutHeight,
      );
      const last = lastPreferenceSize.current;
      if (
        Math.abs(width - last.width) <= 1 &&
        Math.abs(height - last.height) <= 1
      ) {
        return;
      }
      lastPreferenceSize.current = { width, height };
      void resizePreferencesToContent(width, height);
    };
    const scheduleMeasure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    };

    scheduleMeasure();

    return () => {
      cancelAnimationFrame(frame);
    };
  }, [
    isTrayView,
    accounts.length,
    snapshots,
    summary.label,
    busy,
    error,
    activeId,
    form.provider,
    form.secretStorage,
  ]);

  useLayoutEffect(() => {
    if (!isTrayView) {
      return;
    }
    const panel = document.querySelector<HTMLElement>(".tray-panel");
    const scroll = document.querySelector<HTMLElement>(".tray-scroll");
    if (!panel || !scroll) {
      return;
    }

    let frame = 0;
    let resizeObserver: ResizeObserver | null = null;
    let fontReadyCancelled = false;
    const measure = () => {
      const px = (value: string) => Number.parseFloat(value) || 0;
      const style = window.getComputedStyle(panel);
      const paddingY = px(style.paddingTop) + px(style.paddingBottom);
      const rowGap = px(style.rowGap || style.gap);
      const header = panel.querySelector<HTMLElement>(".tray-header");
      const notice = panel.querySelector<HTMLElement>(".tray-notice");
      // The panel and scroll region are fixed to the native window height.
      // Measure the scroll region's children rather than scroll.scrollHeight so
      // a flexed viewport can still shrink when there is little content.
      const scrollStyle = window.getComputedStyle(scroll);
      const scrollGap = px(scrollStyle.rowGap || scrollStyle.gap);
      const scrollChildren = Array.from(scroll.children) as HTMLElement[];
      const scrollContentHeight =
        scrollChildren.reduce((sum, child) => sum + child.offsetHeight, 0) +
        scrollGap * Math.max(0, scrollChildren.length - 1);
      const visibleRows = [header, notice, scroll].filter(Boolean).length;
      const height = Math.ceil(
        paddingY +
          (header?.offsetHeight ?? 0) +
          (notice?.offsetHeight ?? 0) +
          scrollContentHeight +
          rowGap * Math.max(0, visibleRows - 1),
      );
      const scale = Math.max(
        TRAY_MIN_SCALE,
        Math.min(TRAY_MAX_SCALE, settings.trayScale),
      );
      panel.style.setProperty("--tray-scale", scale.toFixed(3));
      const scaledHeight = Math.ceil(height * scale);
      // Keep the tray at the native menu-sized width. Height is adaptive and the
      // internal list scrolls when scaled content exceeds the comfort cap.
      const width = TRAY_BASE_WIDTH;
      if (scaledHeight <= 0 || width <= 0) {
        return;
      }
      const last = lastTraySize.current;
      if (
        Math.abs(width - last.width) <= 1 &&
        Math.abs(scaledHeight - last.height) <= 1
      ) {
        return;
      }
      lastTraySize.current = { width, height: scaledHeight };
      void resizeTrayToContent({ width, height: scaledHeight });
    };
    const scheduleMeasure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    };

    scheduleMeasure();

    if ("ResizeObserver" in window) {
      resizeObserver = new ResizeObserver(scheduleMeasure);
      resizeObserver.observe(panel);
      resizeObserver.observe(scroll);
      for (const child of Array.from(scroll.children)) {
        resizeObserver.observe(child);
      }
    }

    void document.fonts?.ready.then(() => {
      if (!fontReadyCancelled) {
        scheduleMeasure();
      }
    });

    return () => {
      fontReadyCancelled = true;
      cancelAnimationFrame(frame);
      resizeObserver?.disconnect();
    };
  }, [
    isTrayView,
    snapshots,
    error,
    busy,
    accounts,
    summary.label,
    settings.trayScale,
  ]);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    const endpoint = form.endpointOverride?.trim() || null;
    const defaultEndpoint =
      form.provider === "openrouter"
        ? OPENROUTER_DEFAULT_ENDPOINT
        : form.provider === "runpod"
          ? RUNPOD_DEFAULT_ENDPOINT
          : null;
    const endpointOverride =
      defaultEndpoint !== null && endpoint === defaultEndpoint
        ? null
        : endpoint;
    setBusy(true);
    setError(null);
    try {
      const accounts = await saveAccount({
        ...form,
        endpointOverride,
        secret: form.provider === "aws" ? null : form.secret?.trim() || null,
        awsProfile: form.awsProfile?.trim() || null,
        awsRegion: form.awsRegion?.trim() || null,
        awsCategories: form.awsCategories ?? [],
      });
      updateAccounts(accounts, settings, summary);
      setForm(emptyForm);
      setActiveId(null);
      const dashboard = await guardedFetch({ force: true });
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function editAccount(account: AccountView) {
    setActiveId(account.id);
    setForm({
      id: account.id,
      provider: account.provider,
      label: account.label,
      enabled: account.enabled,
      endpointOverride:
        account.endpointOverride ??
        (account.provider === "openrouter"
          ? OPENROUTER_DEFAULT_ENDPOINT
          : account.provider === "runpod"
            ? RUNPOD_DEFAULT_ENDPOINT
            : ""),
      secretStorage: account.secretStorage,
      secret: "",
      awsProfile: account.awsProfile ?? null,
      awsRegion: account.awsRegion ?? "us-east-1",
      awsMonthlyBudgetUsd: account.awsMonthlyBudgetUsd ?? null,
      awsCategories:
        account.provider === "aws"
          ? account.awsCategories?.length
            ? account.awsCategories
            : cloneDefaultAwsCategories()
          : [],
    });
  }

  // After an account mutation, re-fetch and re-cache the dashboard so the
  // persisted cache can't resurrect the old account list / a removed account's
  // snapshot on the next reload (mirrors the post-save flow in onSubmit).
  async function applyAccountChange(accounts: AccountView[]) {
    updateAccounts(accounts, settings, summary);
    const dashboard = await guardedFetch({ force: true });
    setState(dashboard);
    setSnapshots(dashboard.snapshots);
  }

  async function onRemove(id: string) {
    setBusy(true);
    setError(null);
    try {
      await applyAccountChange(await removeAccount(id));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onDetect() {
    setBusy(true);
    setError(null);
    try {
      await applyAccountChange(await detectAccounts());
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function updateAccounts(
    accounts: AccountView[],
    settings: AppSettings,
    summary: ReturnType<typeof summaryFromBackend>,
  ) {
    setState((previous) =>
      previous
        ? { ...previous, accounts }
        : { accounts, snapshots: [], traySummary: summary, settings },
    );
  }

  function startLogin(provider: ProviderKind, accountId?: string) {
    void login.start(provider, providerLabels[provider], accountId);
  }

  function manualAdd(provider: ProviderKind) {
    setActiveId(null);
    setForm({
      provider,
      label: providerLabels[provider],
      enabled: true,
      endpointOverride:
        provider === "openrouter"
          ? OPENROUTER_DEFAULT_ENDPOINT
          : provider === "runpod"
            ? RUNPOD_DEFAULT_ENDPOINT
            : "",
      secretStorage: "keyring",
      secret: "",
      awsProfile: null,
      awsRegion: provider === "aws" ? "us-east-1" : null,
      awsMonthlyBudgetUsd: null,
      awsCategories: provider === "aws" ? cloneDefaultAwsCategories() : [],
    });
  }

  async function onLogout(id: string) {
    setBusy(true);
    setError(null);
    try {
      await applyAccountChange(await logoutAccount(id));
      setForm(emptyForm);
      setActiveId(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  // Optimistically reorder locally, then persist the full id order. The forced
  // re-fetch re-sorts snapshots from the backend and emits a dashboard update so
  // the other window (tray ↔ preferences) stays in sync.
  async function onReorderAccounts(orderedIds: string[]) {
    setState((previous) =>
      previous
        ? {
            ...previous,
            accounts: orderByKey(previous.accounts, orderedIds, (a) => a.id),
          }
        : previous,
    );
    setSnapshots((previous) =>
      orderByKey(previous, orderedIds, (snapshot) => snapshot.accountId),
    );
    try {
      await applyAccountChange(await reorderAccounts(orderedIds));
    } catch (err) {
      setError(String(err));
    }
  }

  if (isTrayView) {
    return (
      <TrayPanel
        state={state}
        snapshots={snapshots}
        busy={busy}
        error={error}
        updateAvailable={updater.state.available}
        onRefresh={() => void revalidate({ force: true })}
        onOpenPreferences={() => void openPreferences()}
        onReorderAccounts={(ids) => void onReorderAccounts(ids)}
      />
    );
  }

  return (
    <>
      <Preferences
        accounts={accounts}
        snapshots={snapshots}
        summary={summary}
        busy={busy}
        error={error}
        form={form}
        activeId={activeId}
        setForm={setForm}
        setActiveId={setActiveId}
        onSubmit={(event) => void onSubmit(event)}
        onDetect={() => void onDetect()}
        onRefresh={() => void revalidate({ force: true })}
        onEditAccount={editAccount}
        onRemoveAccount={(id) => void onRemove(id)}
        onStartLogin={startLogin}
        onManualAdd={manualAdd}
        onLogout={(id) => void onLogout(id)}
        onReorderAccounts={(ids) => void onReorderAccounts(ids)}
        settings={{
          trayScale: settings.trayScale,
          onTrayScaleChange: (scale) => void onTrayScaleChange(scale),
        }}
        updates={{
          channel: settings.updateChannel,
          state: updater.state,
          appVersion,
          onChannelChange: (channel) => void onUpdateChannelChange(channel),
          onCheck: () => void updater.checkNow(),
          onInstall: () => void updater.install(),
          onDismiss: updater.dismiss,
        }}
      />
      {login.session ? (
        <LoginModal
          session={login.session}
          onCancel={() => void login.cancel()}
          onRetry={login.retry}
          onSubmitCode={login.submitCode}
        />
      ) : null}
      <UpdateDialog
        state={updater.state}
        channel={settings.updateChannel}
        appVersion={appVersion}
        onInstall={() => void updater.install()}
        onRetry={() => void updater.checkNow()}
        onClose={updater.closeDialog}
      />
    </>
  );
}

function orderByKey<T>(
  items: T[],
  ids: string[],
  keyOf: (item: T) => string,
): T[] {
  const byKey = new Map(items.map((item) => [keyOf(item), item]));
  const ordered = ids
    .map((id) => byKey.get(id))
    .filter((item): item is T => item !== undefined);
  const rest = items.filter((item) => !ids.includes(keyOf(item)));
  return [...ordered, ...rest];
}

function summaryFromBackend(
  traySummary: DashboardState["traySummary"] | null | undefined,
  snapshots: UsageSnapshot[],
) {
  if (traySummary) {
    return {
      ...traySummary,
      shortLabel: shortSummaryLabel(traySummary.status),
    };
  }

  const criticalCount = snapshots.filter((snapshot) =>
    ["exhausted", "error"].includes(snapshot.status),
  ).length;
  const warningCount = snapshots.filter(
    (snapshot) => snapshot.status === "warning",
  ).length;
  const staleCount = snapshots.filter(
    (snapshot) => snapshot.status === "stale",
  ).length;
  const status =
    criticalCount > 0
      ? "exhausted"
      : warningCount > 0
        ? "warning"
        : staleCount > 0
          ? "stale"
          : snapshots.length > 0
            ? "healthy"
            : "not-configured";
  const label =
    status === "exhausted"
      ? `Burnrate: ${criticalCount} critical`
      : status === "warning"
        ? `Burnrate: ${warningCount} warning`
        : status === "stale"
          ? "Burnrate: data is stale"
          : status === "healthy"
            ? "Burnrate: all quotas healthy"
            : "Burnrate: no enabled accounts";

  return {
    label,
    shortLabel: shortSummaryLabel(status),
    status,
    criticalCount,
    warningCount,
    updatedAt: new Date().toISOString(),
  } as const;
}

function shortSummaryLabel(status: DashboardState["traySummary"]["status"]) {
  switch (status) {
    case "exhausted":
    case "error":
      return "Critical";
    case "warning":
      return "Warning";
    case "stale":
      return "Stale";
    case "not-configured":
      return "Idle";
    case "healthy":
      return "Healthy";
  }
}
