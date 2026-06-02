import {
  FormEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  detectAccounts,
  loadDashboard,
  onDashboardUpdated,
  onRefreshRequested,
  onSettingsUpdated,
  refreshDashboard,
  removeAccount,
  resizePreferencesToContent,
  saveAccount,
  saveSettings,
} from "./api";
import { emptyForm, Preferences } from "./Preferences";
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
    let cleanup: (() => void) | undefined;
    void onRefreshRequested(refreshOnly).then((unlisten) => {
      cleanup = unlisten;
    });
    return () => cleanup?.();
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
    void onDashboardUpdated((dashboard) => {
      setState(dashboard);
      setSnapshots(dashboard.snapshots);
    }).then((unlisten) => {
      cleanupDashboard = unlisten;
    });
    void onSettingsUpdated((settings) => {
      setState((previous) =>
        previous
          ? { ...previous, settings }
          : { accounts: [], snapshots, traySummary: summary, settings },
      );
    }).then((unlisten) => {
      cleanupSettings = unlisten;
    });
    return () => {
      cleanupDashboard?.();
      cleanupSettings?.();
    };
  }, [snapshots, summary]);

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
      const rect = element.getBoundingClientRect();
      const width = Math.ceil(Math.max(element.scrollWidth, rect.width));
      const height = Math.ceil(Math.max(element.scrollHeight, rect.height));
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
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(scheduleMeasure);
    observer?.observe(element);
    window.addEventListener("resize", scheduleMeasure);

    return () => {
      cancelAnimationFrame(frame);
      observer?.disconnect();
      window.removeEventListener("resize", scheduleMeasure);
    };
  }, [
    isTrayView,
    accounts.length,
    snapshots,
    settings.hideFromDock,
    summary.label,
    busy,
    error,
    activeId,
    form.provider,
    form.secretStorage,
  ]);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const accounts = await saveAccount({
        ...form,
        endpointOverride: form.endpointOverride?.trim() || null,
        secret: form.secret?.trim() || null,
      });
      updateAccounts(accounts, settings, summary);
      setForm(emptyForm);
      setActiveId(null);
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
      endpointOverride: account.endpointOverride ?? "",
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

  async function onSettingsChange(settings: AppSettings) {
    setBusy(true);
    setError(null);
    try {
      const nextSettings = await saveSettings(settings);
      setState((previous) =>
        previous
          ? { ...previous, settings: nextSettings }
          : {
              accounts: [],
              snapshots,
              traySummary: summary,
              settings: nextSettings,
            },
      );
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
      settings={settings}
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
      onSettingsChange={(settings) => void onSettingsChange(settings)}
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
