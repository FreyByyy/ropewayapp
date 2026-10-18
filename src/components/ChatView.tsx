import { Fragment, useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { IS_MOBILE } from "../platform";
import { BackButton } from "./MobileNav";
import { api, GENERAL, type ChatMessage, type PeerView, type TransferView } from "../api";
import { useI18n, type Translate } from "../i18n";
import {
  IconAlert,
  IconCast,
  IconCheck,
  IconClock,
  IconFiles,
  IconHash,
  IconPointer,
  IconSend,
  IconShield,
  IconShieldCheck,
  IconSmile,
} from "../icons";
import { Avatar } from "./Avatar";
import { duplicateNames, isDuplicate, shortCode } from "../identity";
import { EmojiPicker } from "./EmojiPicker";
import { TransferCard } from "./TransferCard";

interface Props {
  conversation: string;
  peer?: PeerView;
  /** All known devices, to tell apart devices with the same name. */
  peers: PeerView[];
  messages?: ChatMessage[];
  /** File transfers with this device, shown in the timeline. */
  transfers: TransferView[];
  onTrust: (peer: PeerView) => void;
  /** Opens the "share screen" dialog, for one device or everyone. */
  onShareScreen: (peer?: PeerView) => void;
}

const GROUP_GAP_MS = 5 * 60 * 1000;

export function ChatView({ conversation, peer, peers, messages, transfers, onTrust, onShareScreen }: Props) {
  const { t, lang } = useI18n();
  const isGeneral = conversation === GENERAL;
  const dupNames = duplicateNames(peers);
  const listRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  useLayoutEffect(() => {
    stickToBottom.current = true;
  }, [conversation]);

  useLayoutEffect(() => {
    const el = listRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [messages, transfers.length, conversation]);

  const onScroll = () => {
    const el = listRef.current;
    if (el) stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };

  return (
    <section className="chat">
      <header className="chat-head">
        <BackButton />
        {isGeneral ? (
          <>
            <span className="head-icon">
              <IconHash size={20} />
            </span>
            <div className="head-text">
              <h1>{t("generalChat")}</h1>
              <p>{t("generalChatHint")}</p>
            </div>
            <div className="head-actions">
              <button className="tool" onClick={() => onShareScreen()}>
                <IconCast size={17} />
                <span>{t("showScreen")}</span>
              </button>
            </div>
          </>
        ) : peer ? (
          <>
            <Avatar name={peer.name} id={peer.id} online={peer.online} size={40} />
            <div className="head-text">
              <h1>{peer.name}</h1>
              <p>
                <span className={peer.online ? "status-on" : "status-off"}>
                  {peer.online ? t("online") : t("offline")}
                </span>
                {peer.address && <span className="mono"> · {peer.address}</span>}
                <span className="mono dim"> · {peer.code}</span>
              </p>
              {isDuplicate(dupNames, peer.name) && (
                <p className="same-name-note">
                  <IconAlert size={13} /> {t("sameNameHint")}
                </p>
              )}
            </div>
            <div className="head-actions">
              <FilesMenu peer={peer} />
              <ScreenMenu peer={peer} onShare={() => onShareScreen(peer)} />
              <button
                className="tool"
                disabled={!peer.online}
                title={peer.online ? t("controlHint") : t("offline")}
                onClick={() => api.askScreen(peer.id, true).catch(console.error)}
              >
                <IconPointer size={17} />
                <span>{t("control")}</span>
              </button>
              <span className="divider" />
              <button
                className={`tool trust ${peer.trusted ? "on" : ""}`}
                onClick={() => onTrust(peer)}
                title={peer.trusted ? t("untrust") : t("trust")}
              >
                {peer.trusted ? <IconShieldCheck size={17} /> : <IconShield size={17} />}
                <span>{peer.trusted ? t("trusted") : t("trust")}</span>
              </button>
            </div>
          </>
        ) : null}
      </header>

      <div className="messages" ref={listRef} onScroll={onScroll}>
        {messages && messages.length === 0 && transfers.length === 0 && (
          <div className="empty">
            <p className="empty-title">{t("noMessages")}</p>
            <p className="empty-hint">{t("noMessagesHint")}</p>
          </div>
        )}
        {messages && (
          <MessageList
            messages={messages}
            transfers={transfers}
            showNames={isGeneral}
            peers={peers}
            dupNames={dupNames}
            t={t}
            lang={lang}
          />
        )}
      </div>

      {!isGeneral && peer && !peer.online && <div className="offline-note">{t("offlineNote")}</div>}
      <Composer conversation={conversation} placeholder={isGeneral ? t("generalPlaceholder") : t("messagePlaceholder")} />
    </section>
  );
}

type Item = { kind: "msg"; ts: number; msg: ChatMessage } | { kind: "transfer"; ts: number; transfer: TransferView };

function MessageList({
  messages,
  transfers,
  showNames,
  peers,
  dupNames,
  t,
  lang,
}: {
  messages: ChatMessage[];
  transfers: TransferView[];
  showNames: boolean;
  peers: PeerView[];
  dupNames: Set<string>;
  t: Translate;
  lang: string;
}) {
  const items: Item[] = [
    ...messages.map((msg): Item => ({ kind: "msg", ts: msg.ts, msg })),
    ...transfers.map((transfer): Item => ({ kind: "transfer", ts: transfer.created, transfer })),
  ].sort((a, b) => a.ts - b.ts);
  return (
    <>
      {items.map((item, i) => {
        const prev = items[i - 1];
        const newDay = !prev || dayKey(prev.ts) !== dayKey(item.ts);
        const day = newDay && <div className="day">{dayLabel(item.ts, t, lang)}</div>;
        if (item.kind === "transfer") {
          return (
            <Fragment key={item.transfer.id}>
              {day}
              <TransferCard t={item.transfer} />
            </Fragment>
          );
        }
        const m = item.msg;
        const grouped =
          !newDay &&
          prev?.kind === "msg" &&
          prev.msg.from_id === m.from_id &&
          prev.msg.outgoing === m.outgoing &&
          m.ts - prev.msg.ts < GROUP_GAP_MS;
        return (
          <Fragment key={m.id}>
            {day}
            <Bubble
              msg={m}
              grouped={!!grouped}
              sender={
                showNames && !m.outgoing && !grouped
                  ? {
                      trusted: !!peers.find((p) => p.id === m.from_id)?.trusted,
                      duplicate: isDuplicate(dupNames, m.from_name),
                    }
                  : null
              }
              t={t}
              lang={lang}
            />
          </Fragment>
        );
      })}
    </>
  );
}

function ScreenMenu({ peer, onShare }: { peer: PeerView; onShare: () => void }) {
  const { t } = useI18n();
  const [openMenu, setOpenMenu] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!openMenu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpenMenu(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [openMenu]);

  return (
    <div className="menu-anchor" ref={ref}>
      <button
        className={`tool ${openMenu ? "pressed" : ""}`}
        disabled={!peer.online}
        title={peer.online ? "" : t("offline")}
        onClick={() => setOpenMenu((v) => !v)}
      >
        <IconCast size={17} />
        <span>{t("screen")}</span>
      </button>
      {openMenu && (
        <div className="menu">
          <button
            onClick={() => {
              setOpenMenu(false);
              onShare();
            }}
          >
            {t("screenMenuShow")}
          </button>
          <button
            onClick={() => {
              setOpenMenu(false);
              api.askScreen(peer.id).catch(console.error);
            }}
          >
            {t("screenMenuAsk")}
          </button>
        </div>
      )}
    </div>
  );
}

function FilesMenu({ peer }: { peer: PeerView }) {
  const { t } = useI18n();
  const [openMenu, setOpenMenu] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!openMenu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpenMenu(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [openMenu]);

  const pick = async (directory: boolean) => {
    setOpenMenu(false);
    const selected = IS_MOBILE ? await api.pickFiles() : await open({ multiple: true, directory });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length) await api.sendPaths(peer.id, paths);
  };

  return (
    <div className="menu-anchor" ref={ref}>
      <button className={`tool ${openMenu ? "pressed" : ""}`} onClick={() => setOpenMenu((v) => !v)}>
        <IconFiles size={17} />
        <span>{t("files")}</span>
      </button>
      {openMenu && (
        <div className="menu">
          <button onClick={() => pick(false).catch(console.error)}>{t("sendFiles")}</button>
          {!IS_MOBILE && <button onClick={() => pick(true).catch(console.error)}>{t("sendFolder")}</button>}
        </div>
      )}
    </div>
  );
}

function Bubble({
  msg,
  grouped,
  sender,
  t,
  lang,
}: {
  msg: ChatMessage;
  grouped: boolean;
  /** Shown above the text in the general chat. */
  sender: { trusted: boolean; duplicate: boolean } | null;
  t: Translate;
  lang: string;
}) {
  const time = new Date(msg.ts).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
  return (
    <div className={`row ${msg.outgoing ? "out" : "in"} ${grouped ? "grouped" : ""}`}>
      <div className={`bubble ${msg.status === "failed" ? "failed" : ""}`}>
        {sender && (
          <div className="sender">
            <span className="truncate">{msg.from_name}</span>
            {sender.trusted && <IconShieldCheck size={13} className="trusted-mark" />}
            <span className="sender-code mono">{shortCode(msg.from_id)}</span>
            {sender.duplicate && (
              <span className="same-name" title={t("sameNameHint")}>
                <IconAlert size={12} />
              </span>
            )}
          </div>
        )}
        <div className="text">{msg.text}</div>
        <div className="meta">
          <span>{time}</span>
          {msg.outgoing && <StatusIcon msg={msg} t={t} />}
        </div>
      </div>
      {msg.outgoing && msg.status === "failed" && (
        <button className="retry" onClick={() => api.retryChat(msg.conversation, msg.id)}>
          {t("retry")}
        </button>
      )}
    </div>
  );
}

function StatusIcon({ msg, t }: { msg: ChatMessage; t: Translate }) {
  switch (msg.status) {
    case "pending":
      return <IconClock size={13} aria-label={t("pending")} />;
    case "sent":
      return <IconCheck size={14} aria-label={t("sent")} />;
    case "failed":
      return <IconAlert size={13} className="failed-icon" aria-label={t("failed")} />;
    default:
      return null;
  }
}

function Composer({ conversation, placeholder }: { conversation: string; placeholder: string }) {
  const { t } = useI18n();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const [emojiOpen, setEmojiOpen] = useState(false);
  const closeEmoji = useCallback(() => setEmojiOpen(false), []);
  const text = drafts[conversation] ?? "";

  // Puts the emoji where the cursor is and keeps the cursor after it.
  const insertEmoji = (emoji: string) => {
    const el = ref.current;
    const start = el?.selectionStart ?? text.length;
    const end = el?.selectionEnd ?? text.length;
    setDrafts((d) => ({ ...d, [conversation]: text.slice(0, start) + emoji + text.slice(end) }));
    requestAnimationFrame(() => {
      if (!el) return;
      const caret = start + emoji.length;
      el.setSelectionRange(caret, caret);
    });
  };

  useEffect(() => {
    ref.current?.focus();
    setError(null);
  }, [conversation]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, [text]);

  const send = async () => {
    const value = text.trim();
    if (!value) return;
    setDrafts((d) => ({ ...d, [conversation]: "" }));
    try {
      await api.sendChat(conversation, value);
      setError(null);
    } catch (e) {
      setError(String(e));
      setDrafts((d) => ({ ...d, [conversation]: value }));
    }
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className="composer-wrap">
      {error && <div className="composer-error">{error}</div>}
      {emojiOpen && <EmojiPicker onPick={insertEmoji} onClose={closeEmoji} />}
      <div className="composer">
        <button
          className={`emoji-toggle ${emojiOpen ? "on" : ""}`}
          onClick={() => setEmojiOpen((o) => !o)}
          aria-label={t("emoji")}
          aria-expanded={emojiOpen}
          title={t("emoji")}
        >
          <IconSmile size={20} />
        </button>
        <textarea
          ref={ref}
          rows={1}
          value={text}
          placeholder={placeholder}
          title={t("enterSend")}
          onChange={(e) => setDrafts((d) => ({ ...d, [conversation]: e.target.value }))}
          onKeyDown={onKey}
        />
        <button className="send" onClick={send} disabled={!text.trim()} aria-label={t("send")} title={t("send")}>
          <IconSend size={18} />
        </button>
      </div>
    </div>
  );
}

function dayKey(ts: number) {
  const d = new Date(ts);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

function dayLabel(ts: number, t: Translate, lang: string) {
  const now = new Date();
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (dayKey(ts) === dayKey(now.getTime())) return t("today");
  if (dayKey(ts) === dayKey(yesterday.getTime())) return t("yesterday");
  const d = new Date(ts);
  return d.toLocaleDateString(lang, {
    day: "numeric",
    month: "long",
    year: d.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}
