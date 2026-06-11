import {
  AlertCircle,
  CheckCircle2,
  DownloadCloud,
  RefreshCw,
} from "lucide-react";
import type { UpdateChannel } from "./types";
import type { UpdaterState } from "./useUpdater";

const channelLabels: Record<UpdateChannel, string> = {
  stable: "Burnrate",
  nightly: "Burnrate Nightly",
};

/** The updater's `date` is an opaque timestamp string; render it only when the
 *  platform can actually parse it. */
function formatReleaseDate(value: string | null): string | null {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : parsed.toLocaleDateString();
}

/**
 * Manual-check result dialog. Opened by {@link useUpdater.checkNow} (the tray
 * menu entry and the Preferences "Check for updates" button); the phase is
 * derived from the updater state rather than tracked separately:
 * checking → error → available → up-to-date (or "not supported" on builds
 * where the updater never ran).
 */
export function UpdateDialog({
  state,
  channel,
  appVersion,
  onInstall,
  onRetry,
  onClose,
}: {
  state: UpdaterState;
  channel: UpdateChannel;
  appVersion: string;
  onInstall: () => void;
  onRetry: () => void;
  onClose: () => void;
}) {
  if (!state.dialogOpen) {
    return null;
  }

  const product = channelLabels[channel];
  const releaseDate = formatReleaseDate(state.date);

  let content;
  if (state.checking) {
    content = (
      <>
        <header className="login-modal-head">
          <h2>Software Update</h2>
        </header>
        <p className="login-status">
          <RefreshCw size={16} className="spin" />
          <span>Checking for updates…</span>
        </p>
      </>
    );
  } else if (state.error) {
    content = (
      <>
        <header className="login-modal-head">
          <h2>Update check failed</h2>
        </header>
        <p className="login-error">
          <AlertCircle size={16} />
          <span>{state.error}</span>
        </p>
        <div className="login-actions">
          <button type="button" className="primary" onClick={onRetry}>
            <RefreshCw size={14} />
            Try again
          </button>
          <button type="button" className="secondary" onClick={onClose}>
            Close
          </button>
        </div>
      </>
    );
  } else if (state.available) {
    content = (
      <>
        <header className="login-modal-head">
          <DownloadCloud size={20} />
          <h2>
            {product} {state.version} is available
          </h2>
        </header>
        <p className="update-dialog-meta">
          You have v{appVersion}.
          {releaseDate ? ` Released ${releaseDate}.` : ""}
        </p>
        {state.body ? (
          <div className="update-dialog-notes" aria-label="Release notes">
            {state.body}
          </div>
        ) : null}
        <div className="login-actions">
          <button
            type="button"
            className="primary"
            onClick={onInstall}
            disabled={state.downloading}
          >
            <DownloadCloud size={14} />
            {state.downloading
              ? `Installing… ${state.progress}%`
              : "Install & Restart"}
          </button>
          <button
            type="button"
            className="secondary"
            onClick={onClose}
            disabled={state.downloading}
          >
            Later
          </button>
        </div>
      </>
    );
  } else if (state.hasChecked) {
    content = (
      <>
        <header className="login-modal-head">
          <CheckCircle2 size={20} className="update-dialog-ok" />
          <h2>You’re up to date</h2>
        </header>
        <p className="update-dialog-meta">
          Burnrate v{appVersion} is the latest version on the {channel}{" "}
          channel.
        </p>
        <div className="login-actions">
          <button type="button" className="primary" onClick={onClose}>
            OK
          </button>
        </div>
      </>
    );
  } else {
    // The updater never ran: dev / unsigned / non-bundled builds.
    content = (
      <>
        <header className="login-modal-head">
          <h2>Updates aren’t available in this build</h2>
        </header>
        <p className="update-dialog-meta">
          Automatic updates only work in the signed macOS app bundle.
        </p>
        <div className="login-actions">
          <button type="button" className="primary" onClick={onClose}>
            OK
          </button>
        </div>
      </>
    );
  }

  return (
    <div
      className="login-overlay"
      role="presentation"
      onClick={() => {
        if (!state.downloading) onClose();
      }}
    >
      <div
        className="login-modal update-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Software update"
        onClick={(event) => event.stopPropagation()}
      >
        {content}
      </div>
    </div>
  );
}
