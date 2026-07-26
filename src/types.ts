export type AuthType = "password" | "key" | "agent";

export interface Host {
  id: string;
  label: string;
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  keyPath?: string | null;
}

/** Ukuran panel file yang bisa diatur pengguna, diingat antar sesi. */
export interface PanelLayout {
  /** lebar panel keseluruhan, px */
  width: number;
  /** lebar kolom Ukuran, px */
  sizeW: number;
  /** lebar kolom Terakhir diubah, px */
  modW: number;
}

/** Entri Host hasil baca ~/.ssh/config, belum punya id. */
export interface ConfigHost {
  label: string;
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  keyPath: string | null;
}

export interface ConnectParams {
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  password?: string;
  keyPath?: string;
  keyPassphrase?: string;
  cols: number;
  rows: number;
}

/** Identitas server yang dilaporkan backend saat host key belum dipercaya. */
export interface HostKeyInfo {
  host: string;
  port: number;
  keyType: string;
  /** format "SHA256:..." — sama dengan keluaran `ssh-keygen -lf` */
  fingerprint: string;
  /** fingerprint yang tersimpan; ada = key BERUBAH (bahaya), null = host baru */
  stored: string | null;
}

/** Galat dari `ssh_connect` / `panel_open`. */
export type ConnectError =
  | { kind: "hostKey"; info: HostKeyInfo }
  | { kind: "other"; message: string };

export type TabStatus = "connecting" | "open" | "closed" | "error";

/** Perlakuan rahasia setelah diketik: sekali pakai, selama app berjalan, atau permanen di keyring */
export type SaveMode = "once" | "session" | "disk";

// ---- Panel file browser + statistik ----
export interface DirEntry {
  name: string;
  isDir: boolean;
  size: number;
  /** waktu modifikasi terakhir, detik sejak epoch; null jika server tidak melaporkannya */
  modified: number | null;
}

export interface DirListing {
  path: string;
  entries: DirEntry[];
}

export interface DiskUsage {
  mount: string;
  totalKb: number;
  usedKb: number;
}

export interface ServerStats {
  memTotalKb: number;
  memAvailKb: number;
  disks: DiskUsage[];
  battery: { capacity: number; status: string } | null;
  /** rtt ke 1.1.1.1 dalam ms, null = timeout */
  pingCfMs: number | null;
  /** rtt ke 8.8.8.8 dalam ms, null = timeout */
  pingGoogleMs: number | null;
  /** suhu CPU/SoC dalam °C, null jika tidak ada sensor */
  tempC: number | null;
  /** lama server menyala, detik; 0 = tidak terbaca */
  uptimeS: number;
  /** load average 1 menit */
  load1: number | null;
}

export interface Tab {
  tabId: string;
  host: Host;
  /** rahasia (password / passphrase) untuk sesi ini saja, tidak pernah disimpan ke disk */
  secret?: string;
  status: TabStatus;
  /** penghitung agar tombol "sambung ulang" me-remount terminal */
  attempt: number;
}
