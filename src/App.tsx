import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { addPluginListener } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, GENERAL, type PeerView } from "./api";
import { I18n, makeT } from "./i18n";
import { pendingOffers, useRopeway } from "./store";
import { Sidebar } from "./components/Sidebar";
import { ChatView } from "./components/ChatView";
import { SettingsView } from "./components/SettingsView";
import { SharedView } from "./components/SharedView";
import { ScreenBar } from "./components/ScreenBar";
import { ShareDialog } from "./components/ShareDialog";
import { AddIpModal, TrustModal } from "./components/Modals";
import { UpdateModal, WhatsNewModal } from "./components/UpdateModals";
import { useUpdates } from "./updates";
import { NavRail, type RailSection } from "./components/NavRail";
import { IconSetContext } from "./icons";
import { useAppearance } from "./theme/useAppearance";
import { ThemeInstallContext } from "./theme/install";
import { newThemeId, normalizeTheme } from "./theme/normalize";
import { BUILTIN_THEMES } from "./theme/builtin";
import type { ThemeDef } from "./theme/types";
import { Modal } from "./components/Modals";
import { MobileBackContext } from "./components/MobileNav";
import { Viewer } from "./viewer/Viewer";
import { IS_MOBILE } from "./platform";

const COMPACT_KEY = "ropeway.pane.compact";

type View = { kind: "chat"; conversation: string } | { kind: "settings" } | { kind: "shared" };
type ModalState =
  | { kind: "trust"; peer: PeerView }
  | { kind: "ip" }
  | { kind: "share"; preselected: string[]; everyone: boolean }
  | { kind: "install"; theme: ThemeDef }
  | { kind: "install-error" }
  | null;

type ViewerTarget = { peer: string; session: string };

export default function App() {
  const [view, setView] = useState<View>({ kind: "chat", conversation: GENERAL });
  const [modal, setModal] = useState<ModalState>(null);
  // Phone: the list fills the screen until a chat or page is opened over it;
  // a screen being watched covers everything. Both are history entries, so
  // the system Back button closes them.
  const [mobileMain, setMobileMain] = useState(false);
  const [viewer, setViewer] = useState<ViewerTarget | null>(null);
  const layers = useRef({ main: false, viewer: false });
  const active = view.kind === "chat" && (!IS_MOBILE || mobileMain) ? view.conversation : null;
  const { state, dispatch } = useRopeway(active);
  const appearance = useAppearance();
  const updates = useUpdates();
  const [lastChat, setLastChat] = useState(GENERAL);
  const [compact, setCompact] = useState(() => {
    try {
      return localStorage.getItem(COMPACT_KEY) === "1";
    } catch {
      return false;
    }
  });
  const toggleCompact = useCallback(() => {
    setCompact((c) => {
      try {
        localStorage.setItem(COMPACT_KEY, c ? "0" : "1");
      } catch {
        /* not remembered */
      }
      return !c;
    });
  }, []);

  const lang = state.settings?.language ?? "ru";
  const i18n = useMemo(() => ({ t: makeT(lang), lang }), [lang]);

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const [dragging, setDragging] = useState(false);
  const dropTarget = useRef<PeerView | undefined>(undefined);
  const peerForDrop = active && active !== GENERAL ? state.peers.find((p) => p.id === active) : undefined;
  dropTarget.current = peerForDrop;

  // Files dragged from Explorer onto an open device chat are sent to it.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") setDragging(true);
        else if (p.type === "leave") setDragging(false);
        else if (p.type === "drop") {
          setDragging(false);
          const target = dropTarget.current;
          if (target && p.paths.length) api.sendPaths(target.id, p.paths).catch(console.error);
        }
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const closeModal = useCallback(() => setModal(null), []);
  const installTheme = useCallback((path: string) => {
    api
      .readThemeFile(path)
      .then((raw) => {
        const theme = normalizeTheme(raw);
        setModal(theme ? { kind: "install", theme } : { kind: "install-error" });
      })
      .catch(() => setModal({ kind: "install-error" }));
  }, []);
  const showMain = useCallback(() => {
    if (!IS_MOBILE || layers.current.main) return;
    layers.current.main = true;
    history.pushState({ ropeway: "main" }, "");
    setMobileMain(true);
  }, []);
  const goBack = useCallback(() => history.back(), []);

  useEffect(() => {
    if (!IS_MOBILE) return;
    const onPop = () => {
      if (layers.current.viewer) {
        layers.current.viewer = false;
        setViewer(null);
      } else if (layers.current.main) {
        layers.current.main = false;
        setMobileMain(false);
      }
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  // Phone: a screen shared with us opens over the app.
  useEffect(() => {
    if (!IS_MOBILE) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<ViewerTarget>("ropeway-viewer", ({ payload }) => {
      if (!layers.current.viewer) {
        layers.current.viewer = true;
        history.pushState({ ropeway: "viewer" }, "");
      }
      setViewer({ peer: payload.peer, session: payload.session });
    }).then((off) => {
      if (cancelled) off();
      else unlisten = off;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const openView = useCallback(
    (next: View) => {
      setView(next);
      showMain();
    },
    [showMain],
  );
  const openChat = useCallback(
    (conversation: string) => {
      setLastChat(conversation);
      openView({ kind: "chat", conversation });
    },
    [openView],
  );

  // A click on a system notification opens what it was about.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const open = (target: string) => {
      setModal(null);
      if (target === "shared") openView({ kind: "shared" });
      else if (target.startsWith("chat:")) openChat(target.slice(5));
    };
    listen<string>("ropeway-open", ({ payload }) => open(payload)).then((off) => {
      if (cancelled) off();
      else unlisten = off;
    });
    // Android: the notification plugin reports taps to the page.
    let unlistenTap: (() => void) | undefined;
    if (IS_MOBILE) {
      addPluginListener<{ notification?: { extra?: { open?: unknown } } }>("notification", "actionPerformed", (data) => {
        const target = data.notification?.extra?.open;
        if (typeof target === "string" && target) open(target);
      })
        .then((listener) => {
          if (cancelled) listener.unregister();
          else unlistenTap = () => listener.unregister();
        })
        .catch(() => {});
    }
    return () => {
      cancelled = true;
      unlisten?.();
      unlistenTap?.();
    };
  }, [openChat, openView]);

  if (!state.ready || !state.me || !state.settings || !state.folders || !appearance.loaded) {
    return <div className="boot" />;
  }

  const peer = peerForDrop;
  const offers = pendingOffers(state.transfers);
  const requests = state.changes.filter((c) => c.direction === "incoming" && c.status === "pending").length;
  // Phones use one column whatever the theme's layout.
  const layout = IS_MOBILE ? "sidebar" : appearance.theme.layout;
  const section: RailSection = view.kind === "chat" ? "chats" : view.kind;
  const totalUnread =
    Object.values(state.unread).reduce((a, b) => a + b, 0) + Object.values(offers).reduce((a, b) => a + b, 0);
  const viewKey = view.kind === "chat" ? `chat:${view.conversation}` : view.kind;
  const peerTransfers = peer
    ? Object.values(state.transfers).filter((tr) => tr.peer === peer.id && tr.kind === "offer")
    : [];

  return (
    <I18n.Provider value={i18n}>
      <ThemeInstallContext.Provider value={installTheme}>
      <IconSetContext.Provider value={appearance.theme.icons}>
      <MobileBackContext.Provider value={IS_MOBILE ? goBack : null}>
      <div
        className="app"
        data-layout={layout}
        data-compact={layout === "navview" && compact ? "true" : undefined}
        data-pane={IS_MOBILE ? (mobileMain ? "main" : "list") : undefined}
      >
        {layout === "rail" && (
          <NavRail
            section={section}
            unread={totalUnread}
            requests={requests}
            onAddIp={() => setModal({ kind: "ip" })}
            onSelect={(s) =>
              s === "chats" ? openChat(lastChat) : openView(s === "shared" ? { kind: "shared" } : { kind: "settings" })
            }
          />
        )}
        {(layout !== "rail" || view.kind === "chat") && (
        <Sidebar
          variant={layout === "rail" ? "list" : "full"}
          compact={layout === "navview" && compact}
          onToggleCompact={layout === "navview" ? toggleCompact : undefined}
          me={state.me}
          peers={state.peers}
          last={state.last}
          unread={state.unread}
          offers={offers}
          requests={requests}
          selected={IS_MOBILE ? null : active}
          settingsOpen={!IS_MOBILE && view.kind === "settings"}
          sharedOpen={!IS_MOBILE && view.kind === "shared"}
          onSelect={openChat}
          onShared={() => openView({ kind: "shared" })}
          onSettings={() => openView({ kind: "settings" })}
          onAddIp={() => setModal({ kind: "ip" })}
        />
        )}
        {/* Phone: above the list and the chat alike, so asks are always seen. */}
        {IS_MOBILE && <ScreenBar screen={state.screen} peers={state.peers} />}
        <main className="main">
          {!IS_MOBILE && <ScreenBar screen={state.screen} peers={state.peers} />}
          <div className="view" key={viewKey}>
          {view.kind === "settings" ? (
            <SettingsView
              me={state.me}
              settings={state.settings}
              folders={state.folders}
              peers={state.peers}
              dispatch={dispatch}
              appearance={appearance}
              updates={updates}
              onUntrust={(p) => setModal({ kind: "trust", peer: { ...p, trusted: true } })}
            />
          ) : view.kind === "shared" ? (
            <SharedView
              me={state.me}
              peers={state.peers}
              changes={state.changes}
              edits={state.edits}
              transfers={state.transfers}
              folders={state.folders}
              dispatch={dispatch}
              onOpenSettings={() => openView({ kind: "settings" })}
            />
          ) : view.conversation === GENERAL || peer ? (
            <ChatView
              conversation={view.conversation}
              peer={peer}
              peers={state.peers}
              messages={state.messages[view.conversation]}
              transfers={peerTransfers}
              onTrust={(p) => setModal({ kind: "trust", peer: p })}
              onShareScreen={(p) =>
                setModal({ kind: "share", preselected: p ? [p.id] : [], everyone: !p })
              }
            />
          ) : (
            <div className="placeholder">{i18n.t("selectHint")}</div>
          )}
          </div>
        </main>
      </div>
      </MobileBackContext.Provider>

      {viewer && (
        <div className="mobile-viewer">
          <Viewer key={viewer.session} peer={viewer.peer} session={viewer.session} onClose={goBack} />
        </div>
      )}

      {dragging && (
        <div className={`drop-overlay ${peer ? "" : "no-target"}`}>
          <div className="drop-card">
            {peer ? i18n.t("dropHere", { name: peer.name }) : i18n.t("dropNoTarget")}
            {peer && !peer.online && <div className="drop-sub">{i18n.t("dropOffline")}</div>}
          </div>
        </div>
      )}

      {modal?.kind === "trust" && (
        <TrustModal
          peer={modal.peer}
          peers={state.peers}
          myCode={state.me.code}
          onClose={() => {
            closeModal();
            api.settings().then((settings) => dispatch({ type: "settings", settings }));
          }}
        />
      )}
      {modal?.kind === "share" && (
        <ShareDialog
          peers={state.peers}
          preselected={modal.preselected}
          everyone={modal.everyone}
          initialVideo={state.settings.screen_video}
          onClose={closeModal}
        />
      )}
      {modal?.kind === "ip" && (
        <AddIpModal
          onClose={closeModal}
          onConnected={(p) => {
            closeModal();
            openChat(p.id);
          }}
        />
      )}
      {modal?.kind === "install" && (
        <Modal title={i18n.t("installTitle", { name: modal.theme.name })} onClose={closeModal}>
          <p className="modal-text">{i18n.t("installBody")}</p>
          {modal.theme.css?.trim() && <div className="form-error">{i18n.t("installCssWarn")}</div>}
          <div className="modal-actions">
            <button className="btn ghost" onClick={closeModal}>
              {i18n.t("cancel")}
            </button>
            <button
              className="btn primary"
              onClick={() => {
                const theme = modal.theme;
                // Never overwrite a built-in theme.
                const id = BUILTIN_THEMES.some((b) => b.id === theme.id) ? newThemeId() : theme.id;
                closeModal();
                appearance
                  .saveTheme({ ...theme, id })
                  .then(() => appearance.update({ theme: id }))
                  .catch(console.error);
              }}
            >
              {i18n.t("install")}
            </button>
          </div>
        </Modal>
      )}
      {!modal && updates.whatsNew && <WhatsNewModal version={updates.whatsNew} onClose={updates.closeWhatsNew} />}
      {!modal && !updates.whatsNew && updates.offer && <UpdateModal info={updates.offer} updates={updates} />}
      {modal?.kind === "install-error" && (
        <Modal title={i18n.t("installTheme")} onClose={closeModal}>
          <p className="modal-text">{i18n.t("themeInvalid")}</p>
          <div className="modal-actions">
            <button className="btn primary" onClick={closeModal}>
              OK
            </button>
          </div>
        </Modal>
      )}
      </IconSetContext.Provider>
      </ThemeInstallContext.Provider>
    </I18n.Provider>
  );
}
