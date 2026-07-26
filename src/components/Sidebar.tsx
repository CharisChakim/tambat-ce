import { useMemo, useState } from "react";
import type { Host } from "../types";
import Logo from "./Logo";

interface Props {
  hosts: Host[];
  collapsed: boolean;
  onToggleCollapse: () => void;
  onConnect: (host: Host) => void;
  onEdit: (host: Host) => void;
  onDelete: (host: Host) => void;
  onAdd: () => void;
  onImport: () => void;
}

/** Dua huruf untuk lencana rail: inisial dua kata pertama ("web-prod" → "WP"),
 *  atau dua huruf awal kalau cuma satu kata ("bastion" → "BA"). Satu huruf saja
 *  bikin host berbeda tampak identik saat sidebar menciut. */
const badgeOf = (h: Host) => {
  const src = (h.label || h.host).trim();
  if (!src) return "?";
  const words = src.split(/[\s._\-@:/]+/).filter(Boolean);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  return src.slice(0, 2).toUpperCase();
};

/** Warna tetap per tujuan koneksi, supaya dua lencana berhuruf sama tetap
 *  bisa dibedakan sekilas. */
const hueOf = (h: Host) => {
  let n = 7;
  for (const ch of `${h.username}@${h.host}:${h.port}`) {
    n = (n * 31 + ch.charCodeAt(0)) % 360;
  }
  return n;
};

export default function Sidebar({
  hosts,
  collapsed,
  onToggleCollapse,
  onConnect,
  onEdit,
  onDelete,
  onAdd,
  onImport,
}: Props) {
  const [q, setQ] = useState("");

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return hosts;
    return hosts.filter((h) =>
      [h.label, h.host, h.username].some((v) => v.toLowerCase().includes(needle)),
    );
  }, [hosts, q]);

  // Rail sempit: hanya lencana inisial tiap host (tetap bisa diklik untuk
  // menyambung), tombol perluas di atas, dan tombol tambah host di bawah.
  if (collapsed) {
    return (
      <aside className="sidebar sidebar--rail">
        <button className="rail-toggle" title="Perluas daftar host" onClick={onToggleCollapse}>
          ›
        </button>
        <div className="host-list host-list--rail">
          {hosts.map((h) => (
            <button
              key={h.id}
              className="host-rail"
              title={`${h.label || h.host} — sambungkan ke ${h.username}@${h.host}${
                h.port !== 22 ? `:${h.port}` : ""
              }`}
              style={{
                borderColor: `hsl(${hueOf(h)} 45% 38%)`,
                color: `hsl(${hueOf(h)} 65% 74%)`,
              }}
              onClick={() => onConnect(h)}
            >
              {badgeOf(h)}
            </button>
          ))}
        </div>
        <button className="btn btn--primary rail-add" title="Host baru" onClick={onAdd}>
          +
        </button>
      </aside>
    );
  }

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-text">
          <span className="brand-mark">tambat</span>
          <span className="brand-cursor">_</span>
        </span>
        <Logo className="brand-logo" />
        <button className="rail-toggle" title="Ciutkan daftar host" onClick={onToggleCollapse}>
          ‹
        </button>
      </div>

      <input
        className="search"
        placeholder="Cari host…  ( / )"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setQ("");
        }}
        id="host-search"
      />

      <div className="host-list">
        {filtered.length === 0 && (
          <div className="empty">
            {hosts.length === 0
              ? "Belum ada host. Tambahkan tambatan pertamamu."
              : "Tidak ada yang cocok."}
          </div>
        )}
        {filtered.map((h) => (
          <div
            key={h.id}
            className="host-item"
            onClick={() => onConnect(h)}
            title={`Sambungkan ke ${h.username}@${h.host}`}
          >
            <div className="host-main">
              <div className="host-label">{h.label || h.host}</div>
              <div className="host-sub">
                {h.username}@{h.host}
                {h.port !== 22 ? `:${h.port}` : ""}
                <span className="host-auth"> · {h.authType}</span>
              </div>
            </div>
            <div className="host-actions" onClick={(e) => e.stopPropagation()}>
              <button className="icon-btn" title="Ubah" onClick={() => onEdit(h)}>
                ✎
              </button>
              <button
                className="icon-btn icon-btn--danger"
                title="Hapus"
                onClick={() => onDelete(h)}
              >
                ✕
              </button>
            </div>
          </div>
        ))}
      </div>

      <button className="btn btn--primary add-btn" onClick={onAdd}>
        + Host baru
      </button>
      <button
        className="btn import-btn"
        title="Membaca berkas ~/.ssh/config di komputer ini, lalu menawarkan host yang tercatat di sana untuk ditambahkan"
        onClick={onImport}
      >
        Impor host dari komputer ini
      </button>
    </aside>
  );
}
