import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import {
  b64ToBytes,
  connectParamsFor,
  copyToClipboard,
  errText,
  hostKeyError,
  sshConnect,
  sshDisconnect,
  sshResize,
  sshSend,
  strToB64,
} from "../api";
import type { HostKeyInfo, Tab, TabStatus } from "../types";

const TERM_THEME = {
  background: "#0b141a",
  foreground: "#d7e3ea",
  cursor: "#f2b33d",
  cursorAccent: "#0b141a",
  selectionBackground: "#28455780",
  black: "#101c24",
  red: "#e06c5f",
  green: "#4fbf8b",
  yellow: "#f2b33d",
  blue: "#5aa7d8",
  magenta: "#b58fd8",
  cyan: "#56c2c0",
  white: "#d7e3ea",
  brightBlack: "#5d7684",
  brightRed: "#f08a7e",
  brightGreen: "#6fdca8",
  brightYellow: "#ffd07a",
  brightBlue: "#82c3ec",
  brightMagenta: "#d0b0ef",
  brightCyan: "#7fe0de",
  brightWhite: "#f2f8fb",
};

/** OSC 7 dikirim shell sebagai "file://hostname/path/absolut" */
const OSC7_RE = /^file:\/\/[^/]*(\/.*)$/;

interface Props {
  tab: Tab;
  active: boolean;
  onStatus: (tabId: string, status: TabStatus, message?: string) => void;
  /** dipanggil setiap shell melaporkan direktori kerja baru (lihat OSC7_RE) */
  onCwd?: (path: string) => void;
  /** server menolak diverifikasi otomatis: minta keputusan pengguna */
  onHostKey: (tabId: string, info: HostKeyInfo) => void;
}

export default function TermView({ tab, active, onStatus, onCwd, onHostKey }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const connIdRef = useRef<string | null>(null);

  // Satu lifecycle penuh per attempt: buat terminal, konek, dengarkan, bersihkan.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13.5,
      fontFamily:
        "'JetBrains Mono', ui-monospace, 'Cascadia Mono', 'Fira Code', Menlo, monospace",
      theme: TERM_THEME,
      scrollback: 8000,
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(el);
    fit.fit();
    fitRef.current = fit;
    term.focus();

    // Ctrl+C di terminal sudah berarti SIGINT, jadi salin/tempel pakai konvensi
    // terminal: Ctrl+Shift+C / Ctrl+Shift+V. Ctrl+V biasa tetap jalan lewat
    // penanganan event `paste` bawaan xterm. Kembalikan false = jangan teruskan
    // tombolnya ke server.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
      if (e.code === "KeyC") {
        copyToClipboard(term.getSelection());
        return false;
      }
      if (e.code === "KeyV") {
        navigator.clipboard
          ?.readText()
          .then((text) => {
            const id = connIdRef.current;
            if (id && text) sshSend(id, strToB64(text)).catch(() => {});
          })
          .catch(() => {});
        return false;
      }
      // Ctrl+Shift+W = tutup tab, ditangani shortcut global di App.
      if (e.code === "KeyW") return false;
      return true;
    });

    const oscHandler = term.parser.registerOscHandler(7, (data) => {
      const m = OSC7_RE.exec(data);
      if (m) onCwd?.(m[1]);
      return true;
    });

    let disposed = false;
    let unData: UnlistenFn | null = null;
    let unExit: UnlistenFn | null = null;

    // Input ditahan selagi sesi belum siap, lalu dikirim sekaligus. Tanpa ini,
    // apa pun yang diketik sebelum handshake selesai hilang tanpa jejak.
    let pending = "";
    term.onData((data) => {
      const id = connIdRef.current;
      if (id) sshSend(id, strToB64(data)).catch(() => {});
      else pending += data;
    });

    term.writeln(
      `\x1b[38;5;109mtambat →\x1b[0m menghubungkan ke \x1b[1m${tab.host.username}@${tab.host.host}:${tab.host.port}\x1b[0m ...`,
    );

    // Id sesi dibuat di sini supaya listener terpasang SEBELUM koneksi dibuka;
    // tanpa ini, output awal (banner + prompt) hilang karena event terlanjur dikirim.
    const connId = crypto.randomUUID();

    (async () => {
      try {
        unData = await listen<string>(`ssh-data-${connId}`, (e) => {
          term.write(b64ToBytes(e.payload));
        });
        unExit = await listen<string>(`ssh-exit-${connId}`, (e) => {
          connIdRef.current = null;
          term.writeln(`\r\n\x1b[38;5;109mtambat →\x1b[0m ${e.payload}`);
          onStatus(tab.tabId, "closed", e.payload);
        });

        await sshConnect(connId, connectParamsFor(tab, term.cols, term.rows));
        if (disposed) {
          sshDisconnect(connId).catch(() => {});
          return;
        }
        connIdRef.current = connId;
        onStatus(tab.tabId, "open");

        if (pending) {
          sshSend(connId, strToB64(pending)).catch(() => {});
          pending = "";
        }

        term.onResize(({ cols, rows }) => {
          const id = connIdRef.current;
          if (id) sshResize(id, cols, rows).catch(() => {});
        });
      } catch (err) {
        if (disposed) return;
        // Host key belum dipercaya: bukan galat koneksi, tapi keputusan pengguna.
        // App menampilkan dialog fingerprint lalu menyambung ulang bila disetujui.
        const hk = hostKeyError(err);
        if (hk) {
          term.writeln(
            `\r\n\x1b[33mtambat →\x1b[0m identitas server perlu dikonfirmasi sebelum kredensial dikirim.`,
          );
          onStatus(tab.tabId, "error");
          onHostKey(tab.tabId, hk);
          return;
        }
        const msg = errText(err);
        term.writeln(`\r\n\x1b[31mgagal:\x1b[0m ${msg}`);
        onStatus(tab.tabId, "error", msg);
      }
    })();

    const ro = new ResizeObserver(() => {
      if (el.clientWidth > 0 && el.clientHeight > 0) fit.fit();
    });
    ro.observe(el);

    return () => {
      disposed = true;
      ro.disconnect();
      oscHandler.dispose();
      unData?.();
      unExit?.();
      const id = connIdRef.current;
      if (id) sshDisconnect(id).catch(() => {});
      connIdRef.current = null;
      term.dispose();
    };
    // attempt berubah = sambung ulang penuh
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.attempt]);

  // Saat tab kembali aktif, pas-kan ulang ukuran dan fokuskan.
  useEffect(() => {
    if (active) {
      requestAnimationFrame(() => fitRef.current?.fit());
    }
  }, [active]);

  return (
    <div className={"term-pane" + (active ? "" : " term-pane--hidden")}>
      <div className="term-host" ref={containerRef} />
    </div>
  );
}
