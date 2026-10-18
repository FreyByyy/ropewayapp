import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import changelog from "../CHANGELOG.md?raw";
import { api, type UpdateInfo } from "./api";
import type { Language } from "./api";

const SKIP_KEY = "ropeway.update.skip";
const LAST_KEY = "ropeway.lastVersion";
const PENDING_KEY = "ropeway.update.whatsNew";
/** First check a little after start, then every few hours. */
const FIRST_CHECK_MS = 8_000;
const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* not remembered */
  }
}

/**
 * Bullet points for one language from text in the CHANGELOG.md format:
 * `### Русский` / `### English` sections with `- ` items. Text without
 * sections counts for both languages.
 */
export function notesFor(text: string, lang: Language): string[] {
  const heading = lang === "ru" ? /^###\s+Русский/i : /^###\s+English/i;
  const lines = text.split(/\r?\n/);
  const hasSections = lines.some((l) => /^###\s+/.test(l));
  let inside = !hasSections;
  const items: string[] = [];
  for (const line of lines) {
    if (/^###\s+/.test(line)) {
      inside = heading.test(line);
      continue;
    }
    if (inside && /^\s*[-*]\s+/.test(line)) items.push(line.replace(/^\s*[-*]\s+/, "").trim());
  }
  return items;
}

/** The CHANGELOG.md section of a version, or "" if it has none. */
export function changelogSection(version: string): string {
  const parts = changelog.split(/^## /m);
  const part = parts.find((p) => p.split(/\s/)[0] === version);
  return part ?? "";
}

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo; done: number; total: number | null }
  | { kind: "failed"; info: UpdateInfo | null; error: string };

/**
 * Update checks and installs, plus the "what's new" note after an update.
 * `offer` is set when the app should ask the user on its own; the settings
 * page shows `state` and can check at any time.
 */
export function useUpdates() {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const [offer, setOffer] = useState<UpdateInfo | null>(null);
  const [whatsNew, setWhatsNew] = useState<string | null>(null);

  const check = useCallback(async (byHand: boolean) => {
    setState({ kind: "checking" });
    try {
      const info = await api.checkUpdate();
      if (!info) {
        setState({ kind: "latest" });
        return;
      }
      setState({ kind: "available", info });
      if (!byHand && read(SKIP_KEY) !== info.version) setOffer(info);
    } catch (e) {
      // Offline or no release yet: quiet unless asked.
      setState(byHand ? { kind: "failed", info: null, error: String(e) } : { kind: "idle" });
    }
  }, []);

  useEffect(() => {
    const first = window.setTimeout(() => check(false), FIRST_CHECK_MS);
    const every = window.setInterval(() => check(false), CHECK_EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, [check]);

  // After an update: show the new version's notes once.
  useEffect(() => {
    getVersion()
      .then((current) => {
        const last = read(LAST_KEY);
        const pending = read(PENDING_KEY);
        if ((pending === current || (last && last !== current)) && changelogSection(current)) setWhatsNew(current);
        write(LAST_KEY, current);
        write(PENDING_KEY, null);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const off = listen<{ done: number; total: number | null }>("ropeway-update-progress", (e) => {
      setState((s) => (s.kind === "installing" ? { ...s, done: e.payload.done, total: e.payload.total } : s));
    });
    return () => {
      off.then((f) => f());
    };
  }, []);

  const install = useCallback(async (info: UpdateInfo) => {
    setState({ kind: "installing", info, done: 0, total: null });
    write(PENDING_KEY, info.version);
    try {
      // Desktop restarts into the new version; Android opens its installer.
      await api.installUpdate();
      setState({ kind: "available", info });
    } catch (e) {
      write(PENDING_KEY, null);
      setState({ kind: "failed", info, error: String(e) });
    }
  }, []);

  const skip = useCallback((info: UpdateInfo) => {
    write(SKIP_KEY, info.version);
    setOffer(null);
  }, []);

  return {
    state,
    offer,
    whatsNew,
    check: () => check(true),
    install,
    skip,
    later: () => setOffer(null),
    closeWhatsNew: () => setWhatsNew(null),
    showWhatsNew: (version: string) => setWhatsNew(version),
  };
}

export type Updates = ReturnType<typeof useUpdates>;
