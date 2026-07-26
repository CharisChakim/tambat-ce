import type { DiskUsage } from "./types";

/** Batas panjang `akhir` — dibuat pendek supaya awal nama tetap dapat ruang
 *  di panel sempit. */
const TAIL_MAX = 8;
/** Segmen sepanjang ini masih dianggap ekstensi, bukan bagian nama. */
const EXT_MAX = 4;

/** Pecah nama file jadi [awal, akhir] untuk potong-di-tengah lewat CSS: bagian
 *  `awal` yang menyusut dan diberi elipsis, `akhir` selalu tampil utuh — jadi
 *  ekstensi tidak pernah jadi bagian pertama yang hilang.
 *
 *  `akhir` diambil dari ekstensi, bukan sejumlah karakter buta, supaya di panel
 *  sempit tidak memakan ruang yang seharusnya untuk awal nama. Ekstensi ganda
 *  ikut terbawa selama total masih pendek: "cadangan.sql.gz" → [".sql.gz"].
 *  Nama pendek tidak dipecah (`akhir` kosong). */
export function splitName(name: string): [string, string] {
  const dot = name.lastIndexOf(".");
  if (dot > 0 && name.length - dot - 1 <= EXT_MAX) {
    // Ikutkan ekstensi kedua (mis. ".tar" pada ".tar.gz") bila masih muat.
    const dot2 = name.lastIndexOf(".", dot - 1);
    const start =
      dot2 > 0 && dot - dot2 - 1 <= EXT_MAX && name.length - dot2 <= TAIL_MAX ? dot2 : dot;
    // Hanya pecah kalau masih ada sisa nama yang layak ditampilkan.
    if (start >= 5) return [name.slice(0, start), name.slice(start)];
    return [name, ""];
  }
  // Tanpa ekstensi: sisakan sedikit ekor agar akhiran (mis. "-final") terbaca.
  return name.length > 14 ? [name.slice(0, -5), name.slice(-5)] : [name, ""];
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** Format ringkas untuk chip: 3.4G, 512M, 98G */
export function fmtShort(kb: number): string {
  const units = ["K", "M", "G", "T"];
  let v = kb;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)}${units[i]}`;
}

export const BAT_STATUS: Record<string, string> = {
  Charging: "mengisi",
  Discharging: "melepas",
  Full: "penuh",
  "Not charging": "tidak mengisi",
};

export function joinPath(dir: string, name: string) {
  return dir === "/" ? `/${name}` : `${dir}/${name}`;
}

/** Partisi yang memuat `path`: mount dengan prefix terpanjang. */
export function diskOfPath(disks: DiskUsage[], path: string): DiskUsage | null {
  let best: DiskUsage | null = null;
  for (const d of disks) {
    const m = d.mount.endsWith("/") ? d.mount.slice(0, -1) : d.mount;
    if (path === m || m === "" || path.startsWith(m + "/")) {
      if (!best || m.length > best.mount.length) best = d;
    }
  }
  return best ?? disks[0] ?? null;
}

export const usageCls = (pct: number) =>
  pct >= 90 ? "chip-fill--crit" : pct >= 70 ? "chip-fill--warn" : "";

/** Level sinyal dari latensi: 4=sangat bagus … 1=lemah, 0=timeout. */
export function levelOf(ms: number | null): 0 | 1 | 2 | 3 | 4 {
  if (ms === null) return 0;
  if (ms <= 30) return 4;
  if (ms <= 80) return 3;
  if (ms <= 200) return 2;
  return 1;
}

export const SIG_LABEL = ["timeout", "lemah", "sedang", "bagus", "sangat bagus"];

const BULAN = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/** Epoch detik → "8 Jul 2026 14:32"; null → "-". */
export function fmtDate(epochSeconds: number | null): string {
  if (epochSeconds === null) return "-";
  const d = new Date(epochSeconds * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${d.getDate()} ${BULAN[d.getMonth()]} ${d.getFullYear()} ${hh}:${mm}`;
}

/** Ringkas untuk kolom sempit: "26-07-26 22:11" (dd-mm-yy jam:menit); null → "".
 *  Semua bagian berlebar tetap supaya kolom rata. Tanggal lengkap dengan nama
 *  bulan tetap tersedia lewat fmtDate() di tooltip. */
export function fmtDateShort(epochSeconds: number | null): string {
  if (epochSeconds === null) return "";
  const d = new Date(epochSeconds * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getDate())}-${p(d.getMonth() + 1)}-${p(d.getFullYear() % 100)} ${p(
    d.getHours(),
  )}:${p(d.getMinutes())}`;
}

/** "93784 detik" → "1h 2j"; di bawah sehari "2j 3m"; di bawah sejam "42m". */
export function fmtUptime(s: number): string {
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}h ${h}j`;
  if (h > 0) return `${h}j ${m}m`;
  return `${m}m`;
}
