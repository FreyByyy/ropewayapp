import { invoke } from "@tauri-apps/api/core";

export const GENERAL = "general";

export type MessageStatus = "pending" | "sent" | "failed" | "received";

export interface ChatMessage {
  id: string;
  conversation: string;
  from_id: string;
  from_name: string;
  text: string;
  ts: number;
  outgoing: boolean;
  status: MessageStatus;
}

export interface PeerView {
  id: string;
  name: string;
  os: string;
  address: string | null;
  online: boolean;
  trusted: boolean;
  /** Pinned to the top of the device page; grants nothing. */
  favorite: boolean;
  code: string;
}

export interface MeView {
  id: string;
  name: string;
  code: string;
  port: number;
  addresses: string[];
  /** Link with this device's addresses and key, for a QR code. */
  invite: string;
}

export interface QrMatrix {
  text: string;
  width: number;
  /** "1" dark, "0" light, row by row. */
  cells: string;
}

export interface PairingCode {
  code: string;
  /** Unix time, ms. */
  expires: number;
}

export type Language = "ru" | "en";

export type Resolution = "auto" | "native" | "1440" | "1080" | "720";
export type Quality = "economy" | "standard" | "high";

export interface VideoSettings {
  fps: number;
  resolution: Resolution;
  quality: Quality;
}

export const DEFAULT_VIDEO: VideoSettings = { fps: 30, resolution: "auto", quality: "standard" };

export interface Settings {
  device_name: string;
  language: Language;
  autostart: boolean;
  trusted: { id: string; name: string }[];
  manual_peers: string[];
  screen_video: VideoSettings;
  /** Trusted devices may watch the screen without being asked. */
  trusted_view_without_asking: boolean;
  /** Announce this device and find others on its own. */
  discoverable: boolean;
  favorites: { id: string; name: string }[];
  appearance: unknown;
}

export interface UpdateInfo {
  version: string;
  /** Release notes in the CHANGELOG.md format. */
  notes: string;
  /** Ropeway can install it itself; otherwise the releases page is offered. */
  can_install: boolean;
  page: string;
}

export interface ConversationSummary {
  conversation: string;
  last: ChatMessage | null;
}

export type Direction = "incoming" | "outgoing";
export type TransferKind = "offer" | "download" | "edit";
export type TransferStatus =
  | "waiting"
  | "pending"
  | "active"
  | "interrupted"
  | "completed"
  | "rejected"
  | "cancelled"
  | "failed";

export interface TransferView {
  id: string;
  peer: string;
  peer_name: string;
  direction: Direction;
  kind: TransferKind;
  status: TransferStatus;
  file_count: number;
  names: string[];
  root_count: number;
  total: number;
  transferred: number;
  speed: number;
  current: string | null;
  created: number;
  finished: number | null;
  error: string | null;
  open_path: string | null;
}

export type ChangeKind = "replace" | "delete";
export type ChangeStatus = "pending" | "applying" | "applied" | "rejected" | "failed";

export interface ChangeRequest {
  id: string;
  peer: string;
  peer_name: string;
  direction: Direction;
  kind: ChangeKind;
  path: string;
  size: number | null;
  base_modified: number | null;
  conflict: boolean;
  status: ChangeStatus;
  created: number;
  finished: number | null;
  error: string | null;
}

export interface EditView {
  peer: string;
  peer_name: string;
  path: string;
  local: string;
  ready: boolean;
  modified: boolean;
}

export interface SharedEntry {
  path: string;
  size: number;
  modified: number;
  is_dir: boolean;
}

export interface SharedListing {
  enabled: boolean;
  root: string | null;
  entries: SharedEntry[];
}

export interface Folders {
  download_dir: string;
  shared_dir: string;
  sharing_enabled: boolean;
  shared_trusted_only: boolean;
}

export interface CaptureSource {
  kind: "monitor" | "window";
  id: string;
  label: string;
}

export interface CaptureOptions {
  video: VideoSettings;
  source: CaptureSource | null;
  system_audio: boolean;
  microphone: boolean;
}

export interface SourceInfo {
  kind: "monitor" | "window";
  id: string;
  index: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
  process: string;
}

export interface OutgoingShare {
  id: string;
  title: string;
  system_audio: boolean;
  microphone: boolean;
  public: boolean;
  invited: string[];
  viewers: { id: string; name: string }[];
  /** Connected viewers controlling this computer. */
  controllers: { id: string; name: string }[];
  started: number;
}

export interface RemoteShare {
  id: string;
  peer: string;
  peer_name: string;
  title: string;
  audio: boolean;
  started: number;
  requested: boolean;
  /** This device may control the other one. */
  control: boolean;
}

export interface ScreenAsk {
  id: string;
  peer: string;
  peer_name: string;
  created: number;
  /** Wants to control, not only to watch. */
  control: boolean;
}

export type InputEvent =
  | { t: "mouse_move"; x: number; y: number }
  | { t: "mouse_button"; button: number; down: boolean }
  | { t: "wheel"; dx: number; dy: number }
  | { t: "key"; code: string; down: boolean }
  | { t: "text"; text: string }
  | { t: "switch_monitor"; id: string }
  | { t: "set_video"; video: VideoSettings }
  | { t: "release_all" };

export interface MonitorChoice {
  id: string;
  label: string;
  primary: boolean;
}

export interface ScreenView {
  sharing: OutgoingShare[];
  available: RemoteShare[];
  asks: ScreenAsk[];
  /** Devices this one asked to see, waiting for an answer. */
  asked: string[];
}

export type CoreEvent =
  | { kind: "peers"; data: PeerView[] }
  | { kind: "chat"; data: ChatMessage }
  | { kind: "chat_deleted"; data: { conversation: string; ids: string[] } }
  | { kind: "me"; data: MeView }
  | { kind: "transfer"; data: TransferView }
  | { kind: "changes"; data: ChangeRequest[] }
  | { kind: "edits"; data: EditView[] }
  | { kind: "screen"; data: ScreenView }
  | { kind: "notice"; data: unknown };

export const api = {
  me: () => invoke<MeView>("get_me"),
  peers: () => invoke<PeerView[]>("get_peers"),
  settings: () => invoke<Settings>("get_settings"),
  conversations: () => invoke<ConversationSummary[]>("get_conversations"),
  history: (conversation: string) => invoke<ChatMessage[]>("get_history", { conversation }),
  setDeviceName: (name: string) => invoke<void>("set_device_name", { name }),
  setLanguage: (language: Language) => invoke<void>("set_language", { language }),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  setTrusted: (id: string, trusted: boolean) => invoke<void>("set_trusted", { id, trusted }),
  connectAddress: (address: string) => invoke<PeerView>("connect_address", { address }),
  inviteQr: () => invoke<QrMatrix>("invite_qr"),
  startPairingCode: () => invoke<PairingCode>("start_pairing_code"),
  stopPairingCode: () => invoke<void>("stop_pairing_code"),
  connectInvite: (link: string) => invoke<PeerView>("connect_invite", { link }),
  connectCode: (code: string) => invoke<PeerView>("connect_code", { code }),
  setFavorite: (id: string, favorite: boolean) => invoke<void>("set_favorite", { id, favorite }),
  setDiscoverable: (enabled: boolean) => invoke<void>("set_discoverable", { enabled }),
  deleteMessages: (conversation: string, ids: string[], forEveryone: boolean) =>
    invoke<void>("delete_messages", { conversation, ids, forEveryone }),
  clearChat: (conversation: string) => invoke<void>("clear_chat", { conversation }),
  openProjectPage: () => invoke<void>("open_project_page"),
  sendChat: (conversation: string, text: string) => invoke<ChatMessage>("send_chat", { conversation, text }),
  retryChat: (conversation: string, id: string) => invoke<void>("retry_chat", { conversation, id }),
  setActiveConversation: (conversation: string | null) =>
    invoke<void>("set_active_conversation", { conversation }),

  transfers: () => invoke<TransferView[]>("get_transfers"),
  sendPaths: (peer: string, paths: string[]) => invoke<TransferView>("send_paths", { peer, paths }),
  acceptTransfer: (id: string) => invoke<void>("accept_transfer", { id }),
  rejectTransfer: (id: string) => invoke<void>("reject_transfer", { id }),
  cancelTransfer: (id: string) => invoke<void>("cancel_transfer", { id }),
  retryTransfer: (id: string) => invoke<void>("retry_transfer", { id }),
  clearTransfers: (peer: string) => invoke<void>("clear_transfers", { peer }),

  folders: () => invoke<Folders>("get_folders"),
  setDownloadDir: (path: string | null) => invoke<void>("set_download_dir", { path }),
  setSharedDir: (path: string | null) => invoke<void>("set_shared_dir", { path }),
  setSharing: (enabled: boolean) => invoke<void>("set_sharing", { enabled }),
  setSharedTrustedOnly: (enabled: boolean) => invoke<void>("set_shared_trusted_only", { enabled }),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  openReleasesPage: () => invoke<void>("open_releases_page"),
  setTrustedViewWithoutAsking: (enabled: boolean) => invoke<void>("set_trusted_view_without_asking", { enabled }),

  myShared: () => invoke<SharedListing>("get_my_shared"),
  peerShared: (peer: string) => invoke<SharedListing>("get_peer_shared", { peer }),
  downloadShared: (peer: string, path: string) => invoke<TransferView>("download_shared", { peer, path }),
  editShared: (peer: string, path: string) => invoke<TransferView>("edit_shared", { peer, path }),
  edits: () => invoke<EditView[]>("get_edits"),
  discardEdit: (peer: string, path: string) => invoke<void>("discard_edit", { peer, path }),
  changes: () => invoke<ChangeRequest[]>("get_changes"),
  proposeReplace: (peer: string, path: string) => invoke<ChangeRequest>("propose_replace", { peer, path }),
  proposeDelete: (peer: string, path: string) => invoke<ChangeRequest>("propose_delete", { peer, path }),
  approveChange: (id: string) => invoke<void>("approve_change", { id }),
  rejectChange: (id: string) => invoke<void>("reject_change", { id }),

  screenSources: () => invoke<SourceInfo[]>("list_screen_sources"),
  screen: () => invoke<ScreenView>("get_screen"),
  startShare: (options: CaptureOptions, invite: string[], isPublic: boolean) =>
    invoke<string>("start_screen_share", { options, invite, public: isPublic }),
  stopShare: (id: string) => invoke<void>("stop_screen_share", { id }),
  setScreenVideo: (video: VideoSettings) => invoke<void>("set_screen_video", { video }),
  askScreen: (peer: string, control = false) => invoke<void>("ask_screen", { peer, control }),
  sendInput: (event: InputEvent) => invoke<void>("send_input", { event }),
  answerAsk: (id: string, accept: boolean) => invoke<void>("answer_screen_ask", { id, accept }),
  openViewer: (peer: string, session: string) => invoke<void>("open_viewer", { peer, session }),
  stopWatch: () => invoke<void>("stop_watch"),
  /** Android asks the user before every screen capture; elsewhere always true. */
  requestCapture: () => invoke<boolean>("request_capture"),
  /** Android: files from the system picker, copied to plain paths. */
  pickFiles: () => invoke<string[]>("pick_files"),
  /** Phone: remote control needs Ropeway's accessibility service turned on. */
  controlEnabled: () => invoke<boolean>("control_enabled"),
  openControlSettings: () => invoke<void>("open_control_settings"),
  setSystemBars: (color: string, dark: boolean) => invoke<void>("set_system_bars", { color, dark }),

  listThemes: () => invoke<unknown[]>("list_themes"),
  saveTheme: (theme: unknown) => invoke<void>("save_theme", { theme }),
  deleteTheme: (id: string) => invoke<void>("delete_theme", { id }),
  readThemeFile: (path: string) => invoke<unknown>("read_theme_file", { path }),
  exportTheme: (path: string, theme: unknown) => invoke<void>("export_theme", { path, theme }),
  themeFileForSending: (theme: unknown) => invoke<string>("theme_file_for_sending", { theme }),
  getAppearance: () => invoke<unknown>("get_appearance"),
  setAppearance: (appearance: unknown) => invoke<void>("set_appearance", { appearance }),
  windowsAccent: () => invoke<string | null>("windows_accent"),
  setBackdrop: (mica: boolean, dark: boolean) => invoke<void>("set_backdrop", { mica, dark }),
  micaSupported: () => invoke<boolean>("mica_supported"),

  openPath: (path: string) => invoke<void>("open_path", { path }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
};

/** Joins a shared-folder root and a relative "a/b" path for this OS. */
export function joinLocal(root: string, rel: string) {
  const sep = root.includes("\\") ? "\\" : "/";
  return root.replace(/[\\/]+$/, "") + sep + rel.split("/").join(sep);
}
