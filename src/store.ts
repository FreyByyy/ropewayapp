import { useEffect, useReducer, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type ChangeRequest,
  type ChatMessage,
  type CoreEvent,
  type EditView,
  type Folders,
  type MeView,
  type PeerView,
  type ScreenView,
  type Settings,
  type TransferView,
} from "./api";

export interface State {
  ready: boolean;
  me: MeView | null;
  peers: PeerView[];
  settings: Settings | null;
  folders: Folders | null;
  /** Full history, only for conversations that have been opened. */
  messages: Record<string, ChatMessage[]>;
  /** Latest message per conversation, for the sidebar. */
  last: Record<string, ChatMessage>;
  unread: Record<string, number>;
  transfers: Record<string, TransferView>;
  changes: ChangeRequest[];
  edits: EditView[];
  screen: ScreenView;
}

type Action =
  | {
      type: "init";
      me: MeView;
      peers: PeerView[];
      settings: Settings;
      folders: Folders;
      last: Record<string, ChatMessage>;
      transfers: TransferView[];
      changes: ChangeRequest[];
      edits: EditView[];
      screen: ScreenView;
    }
  | { type: "peers"; peers: PeerView[] }
  | { type: "me"; me: MeView }
  | { type: "settings"; settings: Settings }
  | { type: "folders"; folders: Folders }
  | { type: "chat"; msg: ChatMessage; active: string | null }
  | { type: "history"; conversation: string; messages: ChatMessage[] }
  | { type: "read"; conversation: string }
  | { type: "transfer"; transfer: TransferView }
  | { type: "transfers"; transfers: TransferView[] }
  | { type: "changes"; changes: ChangeRequest[] }
  | { type: "edits"; edits: EditView[] }
  | { type: "screen"; screen: ScreenView };

const initial: State = {
  ready: false,
  me: null,
  peers: [],
  settings: null,
  folders: null,
  messages: {},
  last: {},
  unread: {},
  transfers: {},
  changes: [],
  edits: [],
  screen: { sharing: [], available: [], asks: [], asked: [] },
};

function upsert(list: ChatMessage[], msg: ChatMessage): ChatMessage[] {
  const i = list.findIndex((m) => m.id === msg.id);
  if (i === -1) return [...list, msg];
  const next = list.slice();
  next[i] = msg;
  return next;
}

function byId(list: TransferView[]) {
  return Object.fromEntries(list.map((t) => [t.id, t]));
}

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "init":
      return {
        ...state,
        ready: true,
        me: action.me,
        peers: action.peers,
        settings: action.settings,
        folders: action.folders,
        last: action.last,
        transfers: byId(action.transfers),
        changes: action.changes,
        edits: action.edits,
        screen: action.screen,
      };
    case "peers":
      return { ...state, peers: action.peers };
    case "me":
      return { ...state, me: action.me };
    case "settings":
      return { ...state, settings: action.settings };
    case "folders":
      return { ...state, folders: action.folders };
    case "chat": {
      const { msg } = action;
      const conv = msg.conversation;
      const loaded = state.messages[conv];
      const prevLast = state.last[conv];
      const isNew = !loaded?.some((m) => m.id === msg.id) && prevLast?.id !== msg.id;
      const last = !prevLast || prevLast.ts <= msg.ts || prevLast.id === msg.id ? msg : prevLast;
      const countUnread = isNew && !msg.outgoing && conv !== action.active;
      return {
        ...state,
        messages: loaded ? { ...state.messages, [conv]: upsert(loaded, msg) } : state.messages,
        last: { ...state.last, [conv]: last },
        unread: countUnread ? { ...state.unread, [conv]: (state.unread[conv] ?? 0) + 1 } : state.unread,
      };
    }
    case "history":
      return { ...state, messages: { ...state.messages, [action.conversation]: action.messages } };
    case "read":
      if (!state.unread[action.conversation]) return state;
      return { ...state, unread: { ...state.unread, [action.conversation]: 0 } };
    case "transfer":
      return { ...state, transfers: { ...state.transfers, [action.transfer.id]: action.transfer } };
    case "transfers":
      return { ...state, transfers: byId(action.transfers) };
    case "changes":
      return { ...state, changes: action.changes };
    case "edits":
      return { ...state, edits: action.edits };
    case "screen":
      return { ...state, screen: action.screen };
  }
}

export function useRopeway(active: string | null) {
  const [state, dispatch] = useReducer(reducer, initial);
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const off = await listen<CoreEvent>("ropeway", ({ payload }) => {
        switch (payload.kind) {
          case "peers":
            dispatch({ type: "peers", peers: payload.data });
            break;
          case "me":
            dispatch({ type: "me", me: payload.data });
            break;
          case "chat":
            dispatch({ type: "chat", msg: payload.data, active: activeRef.current });
            break;
          case "transfer":
            dispatch({ type: "transfer", transfer: payload.data });
            break;
          case "changes":
            dispatch({ type: "changes", changes: payload.data });
            break;
          case "edits":
            dispatch({ type: "edits", edits: payload.data });
            break;
          case "screen":
            dispatch({ type: "screen", screen: payload.data });
            break;
        }
      });
      if (cancelled) {
        off();
        return;
      }
      unlisten = off;
      const [me, peers, settings, folders, conversations, transfers, changes, edits, screen] = await Promise.all([
        api.me(),
        api.peers(),
        api.settings(),
        api.folders(),
        api.conversations(),
        api.transfers(),
        api.changes(),
        api.edits(),
        api.screen(),
      ]);
      const last: Record<string, ChatMessage> = {};
      for (const c of conversations) if (c.last) last[c.conversation] = c.last;
      if (!cancelled) dispatch({ type: "init", me, peers, settings, folders, last, transfers, changes, edits, screen });
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    api.setActiveConversation(active).catch(() => {});
    if (!active) return;
    dispatch({ type: "read", conversation: active });
    let cancelled = false;
    api.history(active).then((messages) => {
      if (!cancelled) dispatch({ type: "history", conversation: active, messages });
    });
    return () => {
      cancelled = true;
    };
  }, [active]);

  return { state, dispatch };
}

export type Dispatch = ReturnType<typeof useRopeway>["dispatch"];

/** Incoming offers still waiting for a decision, per device. */
export function pendingOffers(transfers: Record<string, TransferView>) {
  const out: Record<string, number> = {};
  for (const t of Object.values(transfers)) {
    if (t.direction === "incoming" && t.status === "pending") out[t.peer] = (out[t.peer] ?? 0) + 1;
  }
  return out;
}
