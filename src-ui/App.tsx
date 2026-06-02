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
  loadDashboard,
  onDashboardUpdated,
  onRefreshRequested,
  onSettingsUpdated,
  refreshDashboard,
  removeAccount,
  resizePreferencesToContent,
  saveAccount,
} from "./api";
import {
  OPENROUTER_DEFAULT_ENDPOINT,
  emptyForm,
  Preferences,
} from "./Preferences";
import { TrayPanel } from "./TrayPanel";
import type {
  AccountInput,
  AccountView,
  AppSettings,
  DashboardState,
  UsageSnapshot,
} from "./types";

export function App() {
  const isTrayView =
    new URLSearchParams(window.location.search).get("view") === "tray";
  const [state, setState] = useState<DashboardState | null>(null);
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>([]);
  const [form, setForm] = useState<AccountInput>(emptyForm);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const lastPreferenceSize = useRef({ width: 0, height: 0 });

  async function reload() {
    setBusy(true);
    setError(null);
    try {
      const dashboard = await loadDashboard();
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function refreshOnly() {
    setBusy(true);
    setError(null);
    try {
      const dashboard = await refreshDashboard();
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void reload();
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
    void onRefreshRequested(refreshOnly).then((unlisten) => {
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
  const settings = state?.settings ?? { hideFromDock: true };

  useEffect(() => {
    let cleanupDashboard: (() => void) | undefined;
    let cleanupSettings: (() => void) | undefined;
    let disposed = false;
    void onDashboardUpdated((dashboard) => {
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
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

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    const endpoint = form.endpointOverride?.trim() || null;
    const endpointOverride =
      form.provider === "openrouter" && endpoint === OPENROUTER_DEFAULT_ENDPOINT
        ? null
        : endpoint;
    setBusy(true);
    setError(null);
    try {
      const accounts = await saveAccount({
        ...form,
        endpointOverride,
        secret: form.secret?.trim() || null,
      });
      updateAccounts(accounts, settings, summary);
      setForm(emptyForm);
      setActiveId(null);
      const dashboard = await refreshDashboard();
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
        (account.provider === "openrouter" ? OPENROUTER_DEFAULT_ENDPOINT : ""),
      secretStorage: account.secretStorage,
      secret: "",
    });
  }

  async function onRemove(id: string) {
    setBusy(true);
    setError(null);
    try {
      updateAccounts(await removeAccount(id), settings, summary);
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
      updateAccounts(await detectAccounts(), settings, summary);
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

  if (isTrayView) {
    return (
      <TrayPanel
        state={state}
        snapshots={snapshots}
        busy={busy}
        error={error}
        onRefresh={() => void refreshOnly()}
      />
    );
  }

  return (
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
      onRefresh={() => void refreshOnly()}
      onEditAccount={editAccount}
      onRemoveAccount={(id) => void onRemove(id)}
    />
  );
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
