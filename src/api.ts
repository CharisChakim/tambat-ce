import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  ConnectError,
  ConnectParams,
  DirListing,
  Host,
  HostKeyInfo,
  ServerStats,
  Tab,
} from "./types";

/** Galat host key (fingerprint belum dipercaya / berubah), atau null jika bukan itu. */
export function hostKeyError(e: unknown): HostKeyInfo | null {
  const ce = e as ConnectError | null;
  return ce && typeof ce === "object" && ce.kind === "hostKey" ? ce.info : null;
}

/** Pesan galat yang bisa dibaca, dari `String` maupun `ConnectError`. */
export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  const ce = e as ConnectError | null;
  if (ce && typeof ce === "object" && ce.kind === "other") return ce.message;
  if (ce && typeof ce === "object" && ce.kind === "hostKey")
    return `Host key ${ce.info.host} belum dipercaya`;
  return String(e);
}

/** Parameter koneksi dari sebuah tab: kredensial yang dikirim tergantung authType-nya. */
export function connectParamsFor(tab: Tab, cols: number, rows: number): ConnectParams {
  return {
    host: tab.host.host,
    port: tab.host.port,
    username: tab.host.username,
    authType: tab.host.authType,
    password: tab.host.authType === "password" ? tab.secret : undefined,
    keyPath: tab.host.keyPath ?? undefined,
    keyPassphrase: tab.host.authType === "key" ? tab.secret : undefined,
    cols,
    rows,
  };
}

// ---- SSH ----
export const sshConnect = (id: string, params: ConnectParams) =>
  invoke<void>("ssh_connect", { id, params });

export const sshSend = (id: string, dataB64: string) =>
  invoke<void>("ssh_send", { id, dataB64 });

export const sshResize = (id: string, cols: number, rows: number) =>
  invoke<void>("ssh_resize", { id, cols, rows });

export const sshDisconnect = (id: string) =>
  invoke<void>("ssh_disconnect", { id });

// ---- Panel (SFTP + statistik) ----
export const panelOpen = (id: string, params: ConnectParams) =>
  invoke<void>("panel_open", { id, params });

export const panelList = (id: string, path: string) =>
  invoke<DirListing>("panel_list", { id, path });

export const panelStats = (id: string) =>
  invoke<ServerStats>("panel_stats", { id });

export const panelOpenFile = (id: string, path: string, textEditor = false) =>
  invoke<void>("panel_open_file", { id, path, textEditor });

export const panelTransfer = (id: string, src: string, destDir: string, mv: boolean) =>
  invoke<void>("panel_transfer", { id, src, destDir, mv });

export const panelMkdir = (id: string, dir: string, name: string) =>
  invoke<void>("panel_mkdir", { id, dir, name });

export const panelRename = (id: string, src: string, newName: string) =>
  invoke<void>("panel_rename", { id, src, newName });

export const panelDelete = (id: string, path: string) =>
  invoke<void>("panel_delete", { id, path });

/** Unduh ke folder Unduhan; mengembalikan path lokal hasil unduhan. */
export const panelDownload = (id: string, path: string) =>
  invoke<string>("panel_download", { id, path });

export const panelUpload = (id: string, localPath: string, destDir: string) =>
  invoke<void>("panel_upload", { id, localPath, destDir });

export const panelClose = (id: string) => invoke<void>("panel_close", { id });

/** Buka dialog pilih file bawaan OS (bisa banyak file); null jika pengguna membatalkan. */
export const pickFilesToUpload = () =>
  open({ multiple: true, directory: false }) as Promise<string[] | null>;

// ---- Rahasia tersimpan (keyring sistem) ----
export const secretSet = (id: string, secret: string) =>
  invoke<void>("secret_set", { id, secret });

export const secretGet = (id: string) =>
  invoke<string | null>("secret_get", { id });

export const secretDelete = (id: string) =>
  invoke<void>("secret_delete", { id });

// ---- Host key yang dipercaya ----
export const hostkeyTrust = (info: HostKeyInfo) =>
  invoke<void>("hostkey_trust", {
    host: info.host,
    port: info.port,
    keyType: info.keyType,
    fingerprint: info.fingerprint,
  });

// ---- Hosts ----
export const hostsList = () => invoke<Host[]>("hosts_list");
export const hostsSave = (host: Host) => invoke<Host[]>("hosts_save", { host });
export const hostsDelete = (id: string) =>
  invoke<Host[]>("hosts_delete", { id });

// ---- Clipboard ----
/** Fallback lewat textarea sementara: `navigator.clipboard` butuh secure context
 *  dan tidak selalu tersedia di webview Linux (WebKitGTK). Harus dipanggil dari
 *  dalam event gesture pengguna agar execCommand diizinkan. */
function copyFallback(text: string): void {
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "");
  ta.style.position = "fixed";
  ta.style.top = "-1000px";
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand("copy");
  } finally {
    ta.remove();
  }
}

/** Salin teks ke clipboard sistem, dengan fallback bila Clipboard API tak ada. */
export function copyToClipboard(text: string): void {
  if (!text) return;
  const viaApi = navigator.clipboard?.writeText(text);
  if (viaApi) {
    viaApi.catch(() => copyFallback(text));
    return;
  }
  copyFallback(text);
}

// ---- Base64 <-> bytes ----
export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function strToB64(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

/** Cache rahasia per host, hanya di memori selama aplikasi berjalan. */
export const secretCache = new Map<string, string>();
