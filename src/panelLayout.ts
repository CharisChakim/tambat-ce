import type { PanelLayout } from "./types";

export const DEFAULT_LAYOUT: PanelLayout = { width: 320, sizeW: 64, modW: 104 };

/** Batas tetap tiap ukuran, px: [min, maks]. */
const LIMIT: Record<keyof PanelLayout, readonly [number, number]> = {
  width: [240, 720],
  sizeW: [44, 140],
  modW: [70, 220],
};

/** Ruang yang selalu disisakan untuk kolom nama (termasuk ikon, padding, gap).
 *  Tanpa ini kolom nama bisa tergerus habis saat kolom lain dilebarkan sampai
 *  hanya sisa ekor nama yang terlihat. */
const NAME_MIN = 150;

/** Batas efektif satu ukuran: batas tetapnya, dipersempit agar kolom nama tetap
 *  dapat NAME_MIN. Konsekuensinya, melebarkan kolom butuh melebarkan panel dulu. */
export function boundsFor(key: keyof PanelLayout, l: PanelLayout): [number, number] {
  const [lo, hi] = LIMIT[key];
  if (key === "width") return [Math.max(lo, NAME_MIN + l.sizeW + l.modW), hi];
  const other = key === "sizeW" ? l.modW : l.sizeW;
  return [lo, Math.max(lo, Math.min(hi, l.width - NAME_MIN - other))];
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Perbaiki nilai apa pun (termasuk dari localStorage versi lama atau rusak)
 *  menjadi layout yang sah. */
export function normalizeLayout(raw: unknown): PanelLayout {
  const src = (raw ?? {}) as Partial<Record<keyof PanelLayout, unknown>>;
  const pick = (key: keyof PanelLayout) =>
    typeof src[key] === "number" && Number.isFinite(src[key])
      ? clamp(src[key] as number, ...LIMIT[key])
      : DEFAULT_LAYOUT[key];

  const width = pick("width");
  // Kolom dibatasi setelah lebar panel diketahui, supaya invariannya terjaga.
  const half = { ...DEFAULT_LAYOUT, width };
  const sizeW = clamp(pick("sizeW"), ...boundsFor("sizeW", { ...half, modW: 0 }));
  const modW = clamp(pick("modW"), ...boundsFor("modW", { width, sizeW, modW: 0 }));
  return { width, sizeW, modW };
}
