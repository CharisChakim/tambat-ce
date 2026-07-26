import type { HostKeyInfo } from "../types";

interface Props {
  info: HostKeyInfo;
  onTrust: () => void;
  onReject: () => void;
}

/** Konfirmasi identitas server sebelum kredensial dikirim. Dua nada berbeda:
 *  host baru = wajar (cocokkan fingerprint), host key berubah = peringatan keras. */
export default function HostKeyPrompt({ info, onTrust, onReject }: Props) {
  const changed = info.stored !== null;
  const target = `${info.host}${info.port !== 22 ? `:${info.port}` : ""}`;

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onReject()}>
      <div className={"modal" + (changed ? " modal--danger" : "")}>
        <h2 className="modal-title">
          {changed ? "⚠ Host key server BERUBAH" : `Percayai ${target}?`}
        </h2>

        {changed ? (
          <p className="hk-warn">
            Fingerprint <strong>{target}</strong> tidak sama dengan yang tersimpan. Ini bisa
            berarti server dipasang ulang atau key-nya dirotasi — tapi bisa juga berarti ada
            yang menyadap koneksimu. Jangan lanjutkan sebelum kamu tahu pasti penyebabnya.
          </p>
        ) : (
          <p className="hk-note">
            Tambat belum pernah menyambung ke server ini. Cocokkan fingerprint di bawah dengan
            yang kamu dapat langsung dari server (<code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code>).
          </p>
        )}

        <div className="hk-keys">
          <div className="hk-row">
            <span className="hk-label">{changed ? "Sekarang" : "Fingerprint"}</span>
            <code className="hk-fp">{info.fingerprint}</code>
          </div>
          {changed && (
            <div className="hk-row">
              <span className="hk-label">Tersimpan</span>
              <code className="hk-fp hk-fp--old">{info.stored}</code>
            </div>
          )}
          <div className="hk-row">
            <span className="hk-label">Tipe key</span>
            <code className="hk-fp">{info.keyType}</code>
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn" onClick={onReject}>
            Batalkan koneksi
          </button>
          <button
            className={"btn " + (changed ? "btn--danger" : "btn--primary")}
            onClick={onTrust}
          >
            {changed ? "Saya paham, ganti key tersimpan" : "Percaya & sambungkan"}
          </button>
        </div>
      </div>
    </div>
  );
}
