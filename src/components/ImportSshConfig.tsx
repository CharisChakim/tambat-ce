import { useEffect, useMemo, useState } from "react";
import { errText, sshconfigHosts } from "../api";
import type { ConfigHost, Host } from "../types";

interface Props {
  /** host yang sudah ada di sidebar, untuk menandai entri duplikat */
  existing: Host[];
  onImport: (hosts: ConfigHost[]) => void;
  onClose: () => void;
}

/** Sudah ada kalau tujuan koneksinya sama persis (host+port+user), bukan labelnya. */
const sameTarget = (a: ConfigHost, b: Host) =>
  a.host === b.host && a.port === b.port && a.username === b.username;

export default function ImportSshConfig({ existing, onImport, onClose }: Props) {
  const [entries, setEntries] = useState<ConfigHost[] | null>(null);
  const [err, setErr] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());

  useEffect(() => {
    sshconfigHosts()
      .then((list) => {
        setEntries(list);
        // Yang belum ada di sidebar dicentang otomatis; duplikat tidak.
        setPicked(
          new Set(
            list
              .filter((e) => !existing.some((h) => sameTarget(e, h)))
              .map((e) => e.label),
          ),
        );
      })
      .catch((e) => setErr(errText(e)));
    // existing hanya dibaca sekali saat dialog dibuka
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (label: string) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return next;
    });

  const chosen = useMemo(
    () => (entries ?? []).filter((e) => picked.has(e.label)),
    [entries, picked],
  );

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal modal--wide">
        <h2 className="modal-title">Impor dari ~/.ssh/config</h2>

        {err && <div className="form-err">{err}</div>}
        {!entries && !err && <p className="modal-text">membaca ~/.ssh/config…</p>}
        {entries?.length === 0 && (
          <p className="modal-text">
            Tidak ada entri Host yang bisa diimpor. Blok berpola wildcard (<code>Host *</code>)
            dilewati karena hanya berisi setelan default, bukan server.
          </p>
        )}

        {entries && entries.length > 0 && (
          <>
            <p className="modal-text">
              <code>Include</code> dan <code>Match</code> tidak diikuti. Hanya HostName, User,
              Port, dan IdentityFile yang dibaca.
            </p>
            <div className="imp-list">
              {entries.map((e) => {
                const dup = existing.some((h) => sameTarget(e, h));
                return (
                  <label key={e.label} className={"imp-row" + (dup ? " imp-row--dup" : "")}>
                    <input
                      type="checkbox"
                      checked={picked.has(e.label)}
                      onChange={() => toggle(e.label)}
                    />
                    <span className="imp-main">
                      <span className="imp-label">{e.label}</span>
                      <span className="imp-sub">
                        {e.username}@{e.host}
                        {e.port !== 22 ? `:${e.port}` : ""}
                        <span className="imp-auth"> · {e.authType}</span>
                        {e.keyPath ? <span className="imp-key"> · {e.keyPath}</span> : null}
                      </span>
                    </span>
                    {dup && <span className="imp-badge">sudah ada</span>}
                  </label>
                );
              })}
            </div>
          </>
        )}

        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            Batal
          </button>
          <button
            className="btn btn--primary"
            disabled={chosen.length === 0}
            onClick={() => onImport(chosen)}
          >
            {chosen.length > 0 ? `Impor ${chosen.length} host` : "Impor"}
          </button>
        </div>
      </div>
    </div>
  );
}
