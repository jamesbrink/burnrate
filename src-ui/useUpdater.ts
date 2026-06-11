import { useCallback, useEffect, useRef, useState } from "react";
import {
  checkForUpdates,
  installUpdate,
  notifyUpdateAvailable,
  onCheckUpdateRequested,
  onUpdateAvailable,
  onUpdateProgress,
  updaterAvailable,
} from "./api";
import type { UpdateChannel, UpdateInfo } from "./types";

/** Background auto-update poll interval (30 min) — well under GitHub's
 *  anonymous API rate limits even with several Burnrate windows alive. */
const CHECK_INTERVAL_MS = 30 * 60 * 1000;

export interface UpdaterState {
  available: boolean;
  version: string | null;
  body: string | null;
  date: string | null;
  downloading: boolean;
  progress: number;
  error: string | null;
  dismissed: boolean;
  /** A check is in flight (drives the Settings spinner). */
  checking: boolean;
  /** At least one check has completed this session (drives "Up to date"). */
  hasChecked: boolean;
  /** The manual-check result dialog is open. */
  dialogOpen: boolean;
}

const INITIAL_STATE: UpdaterState = {
  available: false,
  version: null,
  body: null,
  date: null,
  downloading: false,
  progress: 0,
  error: null,
  dismissed: false,
  checking: false,
  hasChecked: false,
  dialogOpen: false,
};

export interface UseUpdater {
  state: UpdaterState;
  /** Manual check: opens the result dialog; never posts a notification. */
  checkNow: () => Promise<void>;
  /** Download + install the pending update, then the backend restarts. */
  install: () => Promise<void>;
  /** Hide the banner until a newer version appears. */
  dismiss: () => void;
  /** Close the manual-check dialog (the check result is retained). */
  closeDialog: () => void;
}

export interface UseUpdaterOptions {
  /** Test-only: force the background poll to run under jsdom/dev. */
  forcePoll?: boolean;
  /** Default true. The tray window passes false: no poll, no availability
   *  probe, no tray-menu listener — it only mirrors the backend's
   *  `burnrate-update-available` broadcast, so the Preferences window stays
   *  the single active checker. */
  enabled?: boolean;
}

/**
 * Channel-aware updater state machine. Polls every 30 minutes (off in dev so
 * iteration doesn't ping GitHub), exposes a manual check wired to the tray's
 * "Check for Updates" entry, and tracks download progress for the banner.
 *
 * Background-poll-found updates surface as a system notification (the backend
 * dedupes per version per session); manual checks open the result dialog
 * instead. A manual check that joins an in-flight auto check keeps the auto
 * source — worst case one notification alongside the dialog, accepted.
 *
 * `updaterAvailable()` is false on dev / unsigned builds (no pubkey), so the
 * banner stays hidden there instead of erroring on every poll.
 */
export function useUpdater(
  channel: UpdateChannel,
  options: UseUpdaterOptions = {},
): UseUpdater {
  const enabled = options.enabled !== false;
  const [state, setState] = useState<UpdaterState>(() => ({ ...INITIAL_STATE }));
  const stateRef = useRef(state);
  stateRef.current = state;
  const lastDismissedVersion = useRef<string | null>(null);
  const checkInFlight = useRef<Promise<void> | null>(null);
  const channelRef = useRef(channel);
  channelRef.current = channel;

  const update = useCallback((patch: Partial<UpdaterState>) => {
    setState((prev) => ({ ...prev, ...patch }));
  }, []);

  const applyCheckResult = useCallback(
    (info: UpdateInfo | null) => {
      if (info) {
        // Stay dismissed only if it's the same version the user dismissed.
        const dismissed =
          stateRef.current.dismissed &&
          lastDismissedVersion.current === info.version;
        update({
          available: true,
          version: info.version,
          body: info.body,
          date: info.date,
          error: null,
          dismissed,
          checking: false,
          hasChecked: true,
        });
      } else {
        update({
          available: false,
          version: null,
          body: null,
          date: null,
          error: null,
          checking: false,
          hasChecked: true,
        });
      }
    },
    [update],
  );

  const runCheck = useCallback(
    async (source: "auto" | "manual") => {
      if (checkInFlight.current) return checkInFlight.current;
      const run = (async () => {
        if (!(await updaterAvailable().catch(() => false))) return;
        update({ checking: true });
        try {
          const info = await checkForUpdates(channelRef.current);
          applyCheckResult(info);
          if (source === "auto" && info) {
            // Fire-and-forget; the backend dedupes per version per session. A
            // failed notification is benign (it retries next poll) — swallow
            // it rather than surface an unhandled rejection.
            void notifyUpdateAvailable(info.version).catch(() => {});
          }
        } catch (err) {
          update({ error: String(err), checking: false, hasChecked: true });
        }
      })();
      checkInFlight.current = run;
      try {
        await run;
      } finally {
        if (checkInFlight.current === run) checkInFlight.current = null;
      }
    },
    [update, applyCheckResult],
  );

  const checkNow = useCallback(() => {
    // Open before awaiting so the dialog shows its checking phase
    // immediately, even when joining an already in-flight check.
    update({ dialogOpen: true });
    return runCheck("manual");
  }, [runCheck, update]);

  const closeDialog = useCallback(
    () => update({ dialogOpen: false }),
    [update],
  );

  const install = useCallback(async () => {
    if (stateRef.current.downloading) return;
    const version = stateRef.current.version;
    if (!version) return;
    update({ downloading: true, progress: 0, error: null });
    try {
      // Pass the version we're showing so the backend refuses to install a
      // different pending update if a later check swapped it.
      await installUpdate(version);
      // The backend restarts on success; reaching here means a silent failure.
    } catch (err) {
      update({ downloading: false, progress: 0, error: String(err) });
    }
  }, [update]);

  const dismiss = useCallback(() => {
    lastDismissedVersion.current = stateRef.current.version;
    update({ dismissed: true });
  }, [update]);

  // Track download progress.
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onUpdateProgress((percent) => update({ progress: percent })).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
    );
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [update]);

  // Mirror the backend's per-check broadcast. For the active (Preferences)
  // window this is an idempotent re-application of its own result; for the
  // passive tray window it is the sole source of availability state.
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onUpdateAvailable((info) => applyCheckResult(info)).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
    );
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [applyCheckResult]);

  // Manual "Check for Updates" from the tray menu (active window only).
  useEffect(() => {
    if (!enabled) return;
    let cleanup: (() => void) | undefined;
    let disposed = false;
    void onCheckUpdateRequested(() => void checkNow()).then((unlisten) => {
      if (disposed) unlisten();
      else cleanup = unlisten;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [enabled, checkNow]);

  // Background poll (active window only). Off in dev unless a test opts in;
  // re-checks when the channel changes so switching channels surfaces the
  // right feed immediately.
  useEffect(() => {
    if (!enabled) return;
    if (import.meta.env.DEV && !options.forcePoll) return;
    void runCheck("auto");
    const id = window.setInterval(
      () => void runCheck("auto"),
      CHECK_INTERVAL_MS,
    );
    return () => window.clearInterval(id);
  }, [enabled, runCheck, channel, options.forcePoll]);

  return { state, checkNow, install, dismiss, closeDialog };
}
