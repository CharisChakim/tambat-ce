import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  connectParamsFor,
  errText,
  panelClose,
  panelDelete,
  panelDownload,
  panelList,
  panelMkdir,
  panelOpen,
  panelOpenFile,
  panelRename,
  panelStats,
  panelTransfer,
  panelUpload,
  pickFilesToUpload,
} from "../api";
import {
  BAT_STATUS,
  diskOfPath,
  fmtBytes,
  fmtDate,
  fmtDateShort,
  fmtShort,
  fmtUptime,
  joinPath,
  splitName,
} from "../format";
import { boundsFor } from "../panelLayout";
import type { DirEntry, DirListing, PanelLayout, ServerStats, Tab } from "../types";
import FileAskDialog, { type AskState } from "./FileAskDialog";
import FileContextMenu from "./FileContextMenu";
import FileIcon from "./FileIcon";
import { Chip, IC_BAT, IC_DISK, IC_RAM, IC_TEMP, IC_UPTIME, PingChip } from "./StatChips";

const POLL_MS = 5000;

/** Bentuk file dengan lipatan sudut, dipakai ikon unggah & unduh. */
const berkasOutline = (
  <>
    <path
      d="M4 1.6h5.2L13 5.4V13.4a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V2.6a1 1 0 0 1 1-1z"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
    />
    <path d="M9.2 1.6v3.8H13" fill="none" stroke="currentColor" strokeWidth="1.2" />
  </>
);
const IconUpload = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    {berkasOutline}
    <path
      d="M8 12V7.6M6.1 9.2L8 7.3l1.9 1.9"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);
const IconDownload = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    {berkasOutline}
    <path
      d="M8 7.3v4.4M6.1 9.8L8 11.7l1.9-1.9"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);
const IconDelete = (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    {berkasOutline}
    <path
      d="M6.2 9.3l3.6 3.6M9.8 9.3l-3.6 3.6"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
    />
  </svg>
);

interface Props {
  tab: Tab;
  active: boolean;
  /** direktori kerja shell saat ini (dari OSC 7 terminal); panel mengikutinya */
  cwd?: string;
  layout: PanelLayout;
  onLayoutChange: (l: PanelLayout) => void;
}

/** Panel ala MobaXterm: file browser SFTP + statistik server (RAM, disk,
 *  suhu, baterai, ping). Memakai sesi SSH kedua, terpisah dari terminal. */
export default function FilePanel({ tab, active, cwd, layout, onLayoutChange }: Props) {
  const panelId = `panel-${tab.tabId}-${tab.attempt}`;
  const [listing, setListing] = useState<DirListing | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [stats, setStats] = useState<ServerStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  /** null = otomatis (ikuti folder yang sedang dibuka) */
  const [diskSel, setDiskSel] = useState<string | null>(null);
  const [diskMenu, setDiskMenu] = useState(false);
  /** klik kanan: posisi menu + entri yang diklik (null = area kosong) */
  const [menu, setMenu] = useState<{ x: number; y: number; entry: DirEntry | null } | null>(null);
  /** "clipboard" salin/potong file remote, per tab */
  const [clip, setClip] = useState<{ path: string; mv: boolean } | null>(null);
  const [busyMsg, setBusyMsg] = useState<string | null>(null);
  /** dialog kecil: ganti nama / folder baru / konfirmasi hapus */
  const [ask, setAsk] = useState<AskState | null>(null);
  /** pesan sukses sementara (mis. lokasi hasil unduhan) */
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimer = useRef<number | undefined>(undefined);
  const showNotice = (s: string) => {
    setNotice(s);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000);
  };
  useEffect(() => () => window.clearTimeout(noticeTimer.current), []);
  const activeRef = useRef(active);
  activeRef.current = active;
  const listingRef = useRef(listing);
  listingRef.current = listing;
  /** true selagi file diseret di atas panel (drag & drop dari file manager OS) */
  const [dragOver, setDragOver] = useState(false);
  /** entri yang sedang dipilih (klik tunggal); dasar tombol "unduh terpilih" */
  const [selected, setSelected] = useState<DirEntry | null>(null);

  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;
    setListing(null);
    setStats(null);
    setError(null);

    (async () => {
      try {
        await panelOpen(panelId, connectParamsFor(tab, 80, 24));
        if (disposed) {
          panelClose(panelId).catch(() => {});
          return;
        }
        const l = await panelList(panelId, ".");
        if (disposed) return;
        setListing(l);
        setPathInput(l.path);

        const poll = async () => {
          if (!activeRef.current) return;
          try {
            const s = await panelStats(panelId);
            if (!disposed) setStats(s);
          } catch {
            // statistik gagal sesaat (mis. koneksi lambat) — biarkan nilai lama
          }
        };
        await poll();
        if (!disposed) timer = window.setInterval(poll, POLL_MS);
      } catch (e) {
        if (!disposed) setError(errText(e));
      }
    })();

    return () => {
      disposed = true;
      if (timer) window.clearInterval(timer);
      panelClose(panelId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [panelId, retry]);

  const navigate = async (p: string) => {
    setLoading(true);
    try {
      const l = await panelList(panelId, p);
      setListing(l);
      setPathInput(l.path);
      setSelected(null);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  // Ikuti pwd terminal, TAPI hanya ketika terminal benar-benar `cd` (nilai `cwd`
  // berubah). Tanpa penjaga ini, navigasi folder manual di panel (yang mengubah
  // `listing`) akan memicu efek dan menyeret balik ke pwd terminal — bug "buka
  // folder langsung balik ke home".
  const lastCwd = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!cwd || !listing) return;
    if (cwd === lastCwd.current) return; // terminal belum cd sejak terakhir diikuti
    lastCwd.current = cwd;
    if (cwd !== listing.path) navigate(cwd);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd, listing]);

  const openFile = async (en: DirEntry, textEditor = false) => {
    if (!listing) return;
    setBusyMsg(`membuka ${en.name}…`);
    try {
      await panelOpenFile(panelId, joinPath(listing.path, en.name), textEditor);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMsg(null);
    }
  };

  const download = async (en: DirEntry) => {
    if (!listing) return;
    setBusyMsg(`mengunduh ${en.name}…`);
    try {
      const local = await panelDownload(panelId, joinPath(listing.path, en.name));
      setError(null);
      showNotice(`Tersimpan di ${local}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMsg(null);
    }
  };

  /** Unggah satu atau beberapa file lokal ke `destPath` di server. */
  const uploadTo = async (destPath: string, localPaths: string[]) => {
    if (localPaths.length === 0) return;
    for (const p of localPaths) {
      setBusyMsg(`mengunggah ${p.split("/").pop()}…`);
      try {
        await panelUpload(panelId, p, destPath);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    }
    setBusyMsg(null);
    await navigate(destPath);
  };

  const pickAndUpload = async () => {
    if (!listing) return;
    const picked = await pickFilesToUpload().catch(() => null);
    if (picked) await uploadTo(listing.path, picked);
  };

  // Drag & drop file dari file manager OS langsung ke panel; hanya diproses
  // saat tab ini aktif, karena event drag-drop bersifat window-wide (semua
  // instance FilePanel yang ter-mount menerimanya, termasuk tab yang tersembunyi).
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      if (!activeRef.current) return;
      if (e.payload.type === "enter" || e.payload.type === "over") {
        setDragOver(true);
      } else if (e.payload.type === "leave") {
        setDragOver(false);
      } else if (e.payload.type === "drop") {
        setDragOver(false);
        const dest = listingRef.current?.path;
        if (dest) uploadTo(dest, e.payload.paths);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [panelId]);

  /** Eksekusi dialog `ask` (ganti nama / folder baru / hapus). */
  const submitAsk = async () => {
    if (!ask || !listing) return;
    const a = ask;
    setAsk(null);
    setBusyMsg(
      a.kind === "rename"
        ? "mengganti nama…"
        : a.kind === "mkdir"
          ? "membuat folder…"
          : `menghapus ${a.entry.name}…`,
    );
    try {
      if (a.kind === "rename") {
        await panelRename(panelId, joinPath(listing.path, a.entry.name), a.value.trim());
      } else if (a.kind === "mkdir") {
        await panelMkdir(panelId, listing.path, a.value.trim());
      } else {
        await panelDelete(panelId, joinPath(listing.path, a.entry.name));
      }
      setError(null);
      await navigate(listing.path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMsg(null);
    }
  };

  const paste = async () => {
    if (!clip || !listing) return;
    setBusyMsg(clip.mv ? "memindahkan…" : "menyalin…");
    try {
      await panelTransfer(panelId, clip.path, listing.path, clip.mv);
      if (clip.mv) setClip(null);
      setError(null);
      await navigate(listing.path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMsg(null);
    }
  };

  const copyText = (s: string) => {
    navigator.clipboard?.writeText(s).catch(() => {});
  };

  /** Seret salah satu ukuran panel. `sign` -1 untuk pembatas kolom, karena
   *  menariknya ke kiri berarti kolom di kanannya melebar. */
  const startDrag =
    (key: keyof PanelLayout, sign: 1 | -1) => (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const startX = e.clientX;
      const start = layout[key];
      const [min, max] = boundsFor(key, layout);
      const onMove = (ev: MouseEvent) => {
        const next = start + sign * (ev.clientX - startX);
        onLayoutChange({ ...layout, [key]: Math.min(max, Math.max(min, next)) });
      };
      const onUp = () => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        document.body.classList.remove("resizing-col");
      };
      document.body.classList.add("resizing-col");
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    };

  const grip = (
    <div
      className="fpanel-grip"
      title="Tarik untuk mengubah lebar panel"
      onMouseDown={startDrag("width", 1)}
    />
  );

  /** Kolom dibagikan ke header dan semua baris lewat satu custom property. */
  const colStyle = {
    "--cols": `minmax(0, 1fr) ${layout.sizeW}px ${layout.modW}px`,
  } as React.CSSProperties;
  /** Jarak pembatas kolom dari tepi kanan daftar (padding 7px + gap 8px). */
  const gripRightMod = 7 + layout.modW + 4;
  const gripRightSize = gripRightMod + 4 + layout.sizeW + 4;

  if (error && !listing) {
    return (
      <aside className="fpanel" style={{ width: layout.width, minWidth: layout.width }}>
        <div className="fpanel-msg">
          <p className="fpanel-err">{error}</p>
          <button className="btn" onClick={() => setRetry((r) => r + 1)}>
            Coba lagi
          </button>
        </div>
        {grip}
      </aside>
    );
  }

  const memUsed = stats ? stats.memTotalKb - stats.memAvailKb : 0;
  const disk = stats
    ? (diskSel && stats.disks.find((d) => d.mount === diskSel)) ||
      diskOfPath(stats.disks, listing?.path ?? "/")
    : null;

  return (
    <aside className="fpanel" style={{ width: layout.width, minWidth: layout.width }}>
      <div className="fpanel-nav">
        <button
          className="icon-btn"
          title="Kembali (folder induk)"
          disabled={!listing}
          onClick={() => listing && navigate(`${listing.path}/..`)}
        >
          ←
        </button>
        <button className="icon-btn" title="Folder home" onClick={() => navigate(".")}>
          ⌂
        </button>
        <button
          className="icon-btn"
          title="Muat ulang"
          disabled={!listing}
          onClick={() => listing && navigate(listing.path)}
        >
          ⟳
        </button>
        <button
          className="icon-btn icon-btn--svg"
          title="Unggah file…"
          disabled={!listing}
          onClick={pickAndUpload}
        >
          {IconUpload}
        </button>
        <button
          className="icon-btn icon-btn--svg"
          title={
            selected && !selected.isDir
              ? `Unduh ${selected.name}`
              : "Unduh file terpilih (pilih file dulu)"
          }
          disabled={!selected || selected.isDir}
          onClick={() => selected && !selected.isDir && download(selected)}
        >
          {IconDownload}
        </button>
        <button
          className="icon-btn icon-btn--svg icon-btn--danger"
          title={selected ? `Hapus ${selected.name}` : "Hapus item terpilih (pilih dulu)"}
          disabled={!selected}
          onClick={() => selected && setAsk({ kind: "delete", entry: selected })}
        >
          {IconDelete}
        </button>
        {(loading || busyMsg) && <span className="fpanel-busy">{busyMsg ?? "…"}</span>}
      </div>

      <input
        className="fpanel-path"
        value={pathInput}
        placeholder="/path/folder"
        spellCheck={false}
        onChange={(e) => setPathInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") navigate(pathInput.trim() || ".");
        }}
      />
      {error && <div className="fpanel-err fpanel-err--inline">{error}</div>}
      {notice && <div className="fpanel-note">{notice}</div>}

      <div
        className={"fpanel-list" + (dragOver ? " fpanel-list--dragover" : "")}
        style={colStyle}
        onContextMenu={(e) => {
          e.preventDefault();
          if (listing) setMenu({ x: e.clientX, y: e.clientY, entry: null });
        }}
      >
        {!listing && !error && <div className="fpanel-msg">memuat…</div>}
        {listing?.entries.length === 0 && <div className="fpanel-msg">folder kosong</div>}
        {listing && listing.entries.length > 0 && (
          <div className="fentry-head">
            <span>Nama</span>
            <span className="fentry-head-size">Ukuran</span>
            <span className="fentry-head-modified">Terakhir diubah</span>
            <div
              className="col-grip"
              style={{ right: gripRightSize }}
              title="Tarik untuk mengubah lebar kolom Ukuran"
              onMouseDown={startDrag("sizeW", -1)}
            />
            <div
              className="col-grip"
              style={{ right: gripRightMod }}
              title="Tarik untuk mengubah lebar kolom Terakhir diubah"
              onMouseDown={startDrag("modW", -1)}
            />
          </div>
        )}
        {listing?.entries.map((en) => {
          const [nameHead, nameTail] = splitName(en.name);
          return (
          <div
            key={en.name}
            className={
              "fentry" +
              (en.isDir ? " fentry--dir" : " fentry--file") +
              (selected?.name === en.name ? " fentry--sel" : "") +
              (en.name.startsWith(".") ? " fentry--hidden" : "")
            }
            title={en.name}
            onClick={() => setSelected(en)}
            onDoubleClick={() =>
              en.isDir ? navigate(joinPath(listing.path, en.name)) : openFile(en)
            }
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setSelected(en);
              setMenu({ x: e.clientX, y: e.clientY, entry: en });
            }}
          >
            <span className="fentry-main">
              <FileIcon name={en.name} isDir={en.isDir} />
              {/* Dua span: yang pertama menyusut + elipsis, yang kedua selalu
                  utuh — hasilnya nama terpotong di TENGAH, ekstensi tetap terbaca. */}
              <span className="fentry-name">
                <span className="fentry-name-head">{nameHead}</span>
                <span className="fentry-name-tail">{nameTail}</span>
              </span>
            </span>
            <span className="fentry-size">{en.isDir ? "" : fmtBytes(en.size)}</span>
            <span className="fentry-modified" title={fmtDate(en.modified)}>
              {fmtDateShort(en.modified)}
            </span>
          </div>
          );
        })}
      </div>

      {menu && listing && (
        <FileContextMenu
          menu={menu}
          listing={listing}
          clip={clip}
          onClose={() => setMenu(null)}
          navigate={navigate}
          openFile={openFile}
          download={download}
          copyText={copyText}
          setClip={setClip}
          setAsk={setAsk}
          paste={paste}
        />
      )}

      {ask && (
        <FileAskDialog ask={ask} onChange={setAsk} onClose={() => setAsk(null)} onSubmit={submitAsk} />
      )}

      <div className="fpanel-stats">
        {!stats && <div className="fpanel-msg">menunggu statistik…</div>}
        {stats && (
          <>
            <div className="chip-row">
              {stats.memTotalKb > 0 && (
                <Chip
                  icon={IC_RAM}
                  text={`${fmtShort(memUsed)}/${fmtShort(stats.memTotalKb)}`}
                  pct={(memUsed / stats.memTotalKb) * 100}
                  title={`RAM terpakai ${fmtBytes(memUsed * 1024)} dari ${fmtBytes(stats.memTotalKb * 1024)}`}
                />
              )}
              {disk && (
                <Chip
                  icon={IC_DISK}
                  text={
                    <>
                      <span className="chip-mount">{disk.mount}</span>{" "}
                      {`${fmtShort(disk.usedKb)}/${fmtShort(disk.totalKb)}`}
                    </>
                  }
                  pct={(disk.usedKb / disk.totalKb) * 100}
                  title={
                    `Partisi ${disk.mount}: ${fmtBytes(disk.usedKb * 1024)} dari ${fmtBytes(disk.totalKb * 1024)}` +
                    (diskSel ? " (dipilih manual — klik untuk ganti)" : " (otomatis, ikuti folder — klik untuk ganti)")
                  }
                  onClick={() => setDiskMenu((v) => !v)}
                />
              )}
              {stats.tempC !== null && (
                <Chip
                  icon={IC_TEMP}
                  text={`${Math.round(stats.tempC)}°C`}
                  pct={stats.tempC}
                  fillCls={stats.tempC >= 85 ? "chip-fill--crit" : stats.tempC >= 70 ? "chip-fill--warn" : ""}
                  title={`Suhu CPU ${stats.tempC.toFixed(1)}°C`}
                />
              )}
              {stats.battery && (
                <Chip
                  icon={IC_BAT}
                  text={`${stats.battery.capacity}%`}
                  pct={stats.battery.capacity}
                  fillCls={
                    stats.battery.capacity <= 15
                      ? "chip-fill--crit"
                      : stats.battery.capacity <= 35
                        ? "chip-fill--warn"
                        : ""
                  }
                  title={`Baterai ${stats.battery.capacity}% · ${
                    BAT_STATUS[stats.battery.status] ?? stats.battery.status
                  }`}
                />
              )}
              {!stats.battery && stats.uptimeS > 0 && (
                <Chip
                  icon={IC_UPTIME}
                  text={fmtUptime(stats.uptimeS)}
                  title={
                    `Server menyala ${fmtUptime(stats.uptimeS)}` +
                    (stats.load1 !== null ? ` · load 1 menit ${stats.load1}` : "")
                  }
                />
              )}
              <PingChip ms={stats.pingCfMs} />
            </div>

            {diskMenu && (
              <div className="disk-menu">
                <div
                  className={"disk-item" + (diskSel === null ? " disk-item--sel" : "")}
                  onClick={() => {
                    setDiskSel(null);
                    setDiskMenu(false);
                  }}
                >
                  <span className="disk-mount">Otomatis — ikuti folder</span>
                </div>
                {stats.disks.map((d) => (
                  <div
                    key={d.mount}
                    className={"disk-item" + (diskSel === d.mount ? " disk-item--sel" : "")}
                    onClick={() => {
                      setDiskSel(d.mount);
                      setDiskMenu(false);
                    }}
                  >
                    <span className="disk-mount">{d.mount}</span>
                    <span className="disk-usage">
                      {fmtShort(d.usedKb)}/{fmtShort(d.totalKb)} ·{" "}
                      {Math.round((d.usedKb / d.totalKb) * 100)}%
                    </span>
                  </div>
                ))}
              </div>
            )}

          </>
        )}
      </div>
      {grip}
    </aside>
  );
}
