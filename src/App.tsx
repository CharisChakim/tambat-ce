import { useEffect, useRef, useState } from "react";
import Sidebar from "./components/Sidebar";
import Logo from "./components/Logo";
import FilePanel from "./components/FilePanel";
import HostForm from "./components/HostForm";
import HostKeyPrompt from "./components/HostKeyPrompt";
import ImportSshConfig from "./components/ImportSshConfig";
import SecretPrompt from "./components/SecretPrompt";
import TermView from "./components/TermView";
import {
  hostkeyTrust,
  hostsDelete,
  hostsList,
  hostsSave,
  secretCache,
  secretDelete,
  secretGet,
  secretSet,
} from "./api";
import { DEFAULT_LAYOUT, normalizeLayout } from "./panelLayout";
import type {
  ConfigHost,
  Host,
  HostKeyInfo,
  PanelLayout,
  SaveMode,
  Tab,
  TabStatus,
} from "./types";

let tabCounter = 0;
const newTabId = () => `tab-${++tabCounter}`;

const STATUS_DOT: Record<TabStatus, string> = {
  connecting: "dot dot--connecting",
  open: "dot dot--open",
  closed: "dot dot--closed",
  error: "dot dot--error",
};

export default function App() {
  const [hosts, setHosts] = useState<Host[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  const [formHost, setFormHost] = useState<Host | null | undefined>(undefined); // undefined = tertutup
  const [showPanel, setShowPanel] = useState(true);
  /** sidebar daftar host menciut jadi rail sempit (otomatis saat sesi pertama) */
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const prevTabCount = useRef(0);
  /** direktori kerja shell tiap tab, dilaporkan live oleh TermView lewat OSC 7 */
  const [cwd, setCwd] = useState<Record<string, string>>({});
  const [prompt, setPrompt] = useState<{ host: Host; kind: "password" | "passphrase" } | null>(
    null,
  );
  /** konfirmasi fingerprint server yang belum dipercaya, untuk tab tertentu */
  const [hostKey, setHostKey] = useState<{ tabId: string; info: HostKeyInfo } | null>(null);
  const [showImport, setShowImport] = useState(false);
  /** ukuran panel file, dibagi semua tab dan diingat antar sesi */
  const [panelLayout, setPanelLayout] = useState<PanelLayout>(() => {
    try {
      return normalizeLayout(JSON.parse(localStorage.getItem("tambat.panelLayout") ?? ""));
    } catch {
      return DEFAULT_LAYOUT;
    }
  });

  useEffect(() => {
    hostsList().then(setHosts).catch(console.error);
  }, []);

  useEffect(() => {
    localStorage.setItem("tambat.panelLayout", JSON.stringify(panelLayout));
  }, [panelLayout]);

  // Shortcut: "/" fokus ke pencarian, Ctrl+W tutup tab aktif
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inInput =
        (e.target as HTMLElement)?.tagName === "INPUT" ||
        (e.target as HTMLElement)?.tagName === "SELECT" ||
        (e.target as HTMLElement)?.closest?.(".term-host");
      if (e.key === "/" && !inInput) {
        e.preventDefault();
        setSidebarCollapsed(false); // pastikan kotak cari terlihat sebelum difokus
        setTimeout(() => document.getElementById("host-search")?.focus(), 0);
      }
      // Cocokkan lewat e.code, bukan e.key: dengan Shift ditekan, e.key jadi "W"
      // (huruf besar) sehingga perbandingan terhadap "w" tak pernah terpenuhi.
      if (e.code === "KeyW" && e.ctrlKey && e.shiftKey && activeTab) {
        e.preventDefault();
        closeTab(activeTab);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, tabs]);

  // Sidebar menciut sendiri saat sesi pertama dibuka, dan mengembang lagi saat
  // semua tab ditutup. Hanya bereaksi pada transisi tepi (0↔banyak), jadi tidak
  // melawan toggle manual pengguna di antara kedua keadaan itu.
  useEffect(() => {
    const prev = prevTabCount.current;
    const now = tabs.length;
    if (prev === 0 && now > 0) setSidebarCollapsed(true);
    else if (prev > 0 && now === 0) setSidebarCollapsed(false);
    prevTabCount.current = now;
  }, [tabs.length]);

  const openTab = (host: Host, secret?: string) => {
    const tab: Tab = { tabId: newTabId(), host, secret, status: "connecting", attempt: 1 };
    setTabs((t) => [...t, tab]);
    setActiveTab(tab.tabId);
  };

  const connect = async (host: Host) => {
    if (host.authType === "agent") return openTab(host);

    // Urutan sumber rahasia: cache sesi → keyring sistem → tanya pengguna.
    let secret = secretCache.get(host.id);
    if (secret === undefined) {
      const saved = await secretGet(host.id).catch(() => null);
      if (saved !== null) {
        secret = saved;
        secretCache.set(host.id, saved);
      }
    }
    if (secret !== undefined) return openTab(host, secret);
    if (host.authType === "password") return setPrompt({ host, kind: "password" });
    openTab(host); // key: passphrase diminta hanya jika gagal
  };

  const onSecretSubmit = (secret: string, mode: SaveMode) => {
    if (!prompt) return;
    if (mode !== "once") secretCache.set(prompt.host.id, secret);
    if (mode === "disk") secretSet(prompt.host.id, secret).catch(console.error);
    const host = prompt.host;
    setPrompt(null);
    openTab(host, secret);
  };

  const onStatus = (tabId: string, status: TabStatus, message?: string) => {
    setTabs((ts) =>
      ts.map((t) => (t.tabId === tabId ? { ...t, status } : t)),
    );
    // Rahasia tersimpan yang ditolak server = basi (password diganti dsb.):
    // buang dari cache dan keyring agar koneksi berikutnya bertanya ulang.
    const tab = tabs.find((t) => t.tabId === tabId);
    if (status === "error" && tab) {
      const msg = (message ?? "").toLowerCase();
      if (
        (tab.host.authType === "password" && msg.includes("password")) ||
        (tab.host.authType === "key" && msg.includes("key"))
      ) {
        secretCache.delete(tab.host.id);
        secretDelete(tab.host.id).catch(() => {});
      }
    }
  };

  /** Sambung ulang tab dengan kredensial yang sudah ada (remount TermView). */
  const reconnectTab = (tabId: string) =>
    setTabs((ts) =>
      ts.map((t) =>
        t.tabId === tabId ? { ...t, status: "connecting", attempt: t.attempt + 1 } : t,
      ),
    );

  const onHostKeyTrust = async () => {
    if (!hostKey) return;
    const { tabId, info } = hostKey;
    setHostKey(null);
    try {
      await hostkeyTrust(info);
      reconnectTab(tabId);
    } catch (e) {
      console.error(e);
      onStatus(tabId, "error", String(e));
    }
  };

  const retryTab = (tabId: string) => {
    const tab = tabs.find((t) => t.tabId === tabId);
    if (!tab) return;
    if (tab.host.authType === "password" || tab.host.authType === "key") {
      // Minta rahasia lagi (mungkin salah ketik / key butuh passphrase)
      closeTab(tabId);
      secretCache.delete(tab.host.id);
      setPrompt({
        host: tab.host,
        kind: tab.host.authType === "password" ? "password" : "passphrase",
      });
    } else {
      reconnectTab(tabId);
    }
  };

  const closeTab = (tabId: string) => {
    setTabs((ts) => {
      const next = ts.filter((t) => t.tabId !== tabId);
      if (activeTab === tabId) {
        setActiveTab(next.length ? next[next.length - 1].tabId : null);
      }
      return next;
    });
  };

  const saveHost = async (host: Host, secret?: string) => {
    setHosts(await hostsSave(host));
    setFormHost(undefined);
    if (secret === undefined) return; // mode ubah host: tidak langsung connect
    if (secret) secretCache.set(host.id, secret);
    openTab(host, host.authType === "agent" ? undefined : secret);
  };

  const importHosts = async (chosen: ConfigHost[]) => {
    setShowImport(false);
    let list = hosts;
    for (const c of chosen) {
      list = await hostsSave({ ...c, id: crypto.randomUUID() }).catch((e) => {
        console.error(e);
        return list;
      });
    }
    setHosts(list);
  };

  const deleteHost = async (host: Host) => {
    if (!window.confirm(`Hapus host "${host.label}"?`)) return;
    secretCache.delete(host.id);
    secretDelete(host.id).catch(() => {});
    setHosts(await hostsDelete(host.id));
  };

  const active = tabs.find((t) => t.tabId === activeTab);

  return (
    <div className="layout">
      <Sidebar
        hosts={hosts}
        collapsed={sidebarCollapsed}
        onToggleCollapse={() => setSidebarCollapsed((v) => !v)}
        onConnect={connect}
        onAdd={() => setFormHost(null)}
        onImport={() => setShowImport(true)}
        onEdit={(h) => setFormHost(h)}
        onDelete={deleteHost}
      />

      <main className="main">
        {tabs.length > 0 && (
          <div className="tabbar">
            {tabs.map((t) => (
              <div
                key={t.tabId}
                className={"tab" + (t.tabId === activeTab ? " tab--active" : "")}
                onClick={() => setActiveTab(t.tabId)}
              >
                <span className={STATUS_DOT[t.status]} />
                <span className="tab-label">{t.host.label || t.host.host}</span>
                <button
                  className="tab-close"
                  title="Tutup (Ctrl+Shift+W)"
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(t.tabId);
                  }}
                >
                  ✕
                </button>
              </div>
            ))}
            <button
              className={"panel-toggle" + (showPanel ? " panel-toggle--on" : "")}
              title={showPanel ? "Sembunyikan panel file" : "Tampilkan panel file"}
              onClick={() => setShowPanel((v) => !v)}
            >
              ◧
            </button>
          </div>
        )}

        <div className="term-area">
          {tabs.length === 0 && (
            <div className="welcome">
              <Logo className="welcome-logo" />
              <div className="welcome-tag">
                Tambatkan ip mu
                <span className="welcome-sig" aria-hidden="true">
                  <span>_</span>
                  <span>_</span>
                  <span>_</span>
                </span>
              </div>
              <p>Pilih host di sisi kiri untuk menambatkan sesi baru,</p>
              <p>
                atau tekan <kbd>/</kbd> untuk mencari.
              </p>
            </div>
          )}
          {tabs.map((t) => (
            <div
              key={t.tabId}
              className={
                "workspace" + (t.tabId === activeTab ? "" : " workspace--hidden")
              }
            >
              {showPanel && t.status === "open" && (
                <FilePanel
                  tab={t}
                  active={t.tabId === activeTab}
                  cwd={cwd[t.tabId]}
                  layout={panelLayout}
                  onLayoutChange={setPanelLayout}
                />
              )}
              <div className="workspace-term">
                <TermView
                  key={`${t.tabId}-${t.attempt}`}
                  tab={t}
                  active={t.tabId === activeTab}
                  onStatus={onStatus}
                  onCwd={(path) => setCwd((c) => (c[t.tabId] === path ? c : { ...c, [t.tabId]: path }))}
                  onHostKey={(tabId, info) => setHostKey({ tabId, info })}
                />
              </div>
            </div>
          ))}
          {active &&
            (active.status === "closed" || active.status === "error") &&
            hostKey?.tabId !== active.tabId && (
              <div className="term-overlay">
                <button className="btn btn--primary" onClick={() => retryTab(active.tabId)}>
                  Sambung ulang
                </button>
                <button className="btn" onClick={() => closeTab(active.tabId)}>
                  Tutup tab
                </button>
              </div>
            )}
        </div>
      </main>

      {formHost !== undefined && (
        <HostForm
          initial={formHost}
          onSave={saveHost}
          onClose={() => setFormHost(undefined)}
        />
      )}
      {showImport && (
        <ImportSshConfig
          existing={hosts}
          onImport={importHosts}
          onClose={() => setShowImport(false)}
        />
      )}
      {hostKey && (
        <HostKeyPrompt
          info={hostKey.info}
          onTrust={onHostKeyTrust}
          onReject={() => setHostKey(null)}
        />
      )}
      {prompt && (
        <SecretPrompt
          host={prompt.host}
          kind={prompt.kind}
          onSubmit={onSecretSubmit}
          onClose={() => setPrompt(null)}
        />
      )}
    </div>
  );
}
