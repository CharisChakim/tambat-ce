use base64::{engine::general_purpose::STANDARD_NO_PAD as B64NP, Engine as _};
use serde::{Deserialize, Serialize};
use ssh2::{HashType, HostKeyType, Session};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Satu host key yang sudah dipercaya pengguna. Disimpan di berkas milik
/// Tambat sendiri (bukan ~/.ssh/known_hosts) agar app tidak pernah menulis ke
/// konfigurasi OpenSSH pengguna.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    /// SHA256 base64 tanpa padding — format sama dengan keluaran `ssh-keygen -lf`.
    pub fingerprint: String,
}

/// Host key yang perlu dikonfirmasi pengguna, dikirim ke frontend apa adanya.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    /// Fingerprint yang sudah tersimpan. `Some` = key BERUBAH (bahaya),
    /// `None` = host belum pernah dipercaya.
    pub stored: Option<String>,
}

pub enum Verdict {
    Trusted,
    Untrusted(HostKeyInfo),
}

fn store_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("known_hosts.json"))
}

fn read_all(app: &AppHandle) -> Result<Vec<KnownHost>, String> {
    let path = store_file(app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("known_hosts.json rusak: {}", e))
}

/// Tulis lewat berkas sementara lalu rename, supaya daftar tidak separuh
/// tertulis kalau proses mati di tengah penyimpanan.
fn write_all(app: &AppHandle, list: &[KnownHost]) -> Result<(), String> {
    let path = store_file(app)?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn key_type_name(t: HostKeyType) -> &'static str {
    match t {
        HostKeyType::Rsa => "ssh-rsa",
        HostKeyType::Dss => "ssh-dss",
        HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256",
        HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384",
        HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521",
        HostKeyType::Ed25519 => "ssh-ed25519",
        HostKeyType::Unknown => "tidak dikenal",
    }
}

/// Ambil fingerprint SHA256 host key dari sesi yang sudah selesai handshake.
pub(crate) fn fingerprint_of(sess: &Session) -> Result<(String, String), String> {
    let hash = sess
        .host_key_hash(HashType::Sha256)
        .ok_or("Server tidak mengirimkan host key")?;
    let key_type = sess
        .host_key()
        .map(|(_, t)| key_type_name(t))
        .unwrap_or("tidak dikenal");
    Ok((
        key_type.to_string(),
        format!("SHA256:{}", B64NP.encode(hash)),
    ))
}

/// Keputusan percaya/tidak, dipisah dari I/O berkas & sesi agar bisa diuji.
/// Hanya fingerprint yang sama persis dengan yang tersimpan dianggap terpercaya —
/// host tak dikenal dan fingerprint berubah sama-sama ditolak (bedanya cuma
/// `stored`, yang dipakai frontend untuk memilih nada peringatan).
pub(crate) fn decide(
    host: &str,
    port: u16,
    key_type: String,
    fingerprint: String,
    stored: Option<KnownHost>,
) -> Verdict {
    match stored {
        Some(k) if k.fingerprint == fingerprint => Verdict::Trusted,
        other => Verdict::Untrusted(HostKeyInfo {
            host: host.to_string(),
            port,
            key_type,
            fingerprint,
            stored: other.map(|k| k.fingerprint),
        }),
    }
}

/// Bandingkan host key sesi ini dengan yang tersimpan. Dipanggil SETELAH
/// handshake dan SEBELUM auth, supaya kredensial tidak pernah dikirim ke
/// server yang identitasnya belum terbukti.
pub fn verify(
    app: &AppHandle,
    sess: &Session,
    host: &str,
    port: u16,
) -> Result<Verdict, String> {
    let (key_type, fingerprint) = fingerprint_of(sess)?;
    let stored = read_all(app)?
        .into_iter()
        .find(|k| k.host == host && k.port == port);
    Ok(decide(host, port, key_type, fingerprint, stored))
}

/// Percayai host key ini mulai sekarang (menimpa entri lama bila key berganti).
#[tauri::command]
pub fn hostkey_trust(
    app: AppHandle,
    host: String,
    port: u16,
    key_type: String,
    fingerprint: String,
) -> Result<(), String> {
    let mut list = read_all(&app)?;
    let entry = KnownHost {
        host,
        port,
        key_type,
        fingerprint,
    };
    match list
        .iter_mut()
        .find(|k| k.host == entry.host && k.port == entry.port)
    {
        Some(existing) => *existing = entry,
        None => list.push(entry),
    }
    write_all(&app, &list)
}

#[cfg(test)]
mod tests {
    use super::{decide, KnownHost, Verdict};

    fn known(fp: &str) -> KnownHost {
        KnownHost {
            host: "srv".into(),
            port: 22,
            key_type: "ssh-ed25519".into(),
            fingerprint: fp.into(),
        }
    }

    #[test]
    fn host_belum_dikenal_ditolak() {
        match decide("srv", 22, "ssh-ed25519".into(), "SHA256:aaa".into(), None) {
            Verdict::Untrusted(i) => {
                assert_eq!(i.fingerprint, "SHA256:aaa");
                assert_eq!(i.stored, None, "host baru: tak ada fingerprint lama");
            }
            Verdict::Trusted => panic!("host tak dikenal tidak boleh dipercaya"),
        }
    }

    #[test]
    fn fingerprint_sama_dipercaya() {
        assert!(matches!(
            decide(
                "srv",
                22,
                "ssh-ed25519".into(),
                "SHA256:aaa".into(),
                Some(known("SHA256:aaa")),
            ),
            Verdict::Trusted
        ));
    }

    #[test]
    fn fingerprint_berubah_ditolak_dan_bawa_yang_lama() {
        match decide(
            "srv",
            22,
            "ssh-ed25519".into(),
            "SHA256:baru".into(),
            Some(known("SHA256:lama")),
        ) {
            Verdict::Untrusted(i) => {
                assert_eq!(i.fingerprint, "SHA256:baru");
                assert_eq!(
                    i.stored.as_deref(),
                    Some("SHA256:lama"),
                    "dialog perlu fingerprint lama untuk peringatan MITM"
                );
            }
            Verdict::Trusted => panic!("key yang berubah tidak boleh dipercaya diam-diam"),
        }
    }

    /// E2E terhadap mock sshd (tests/mock_sshd.py): fingerprint terbaca, berformat
    /// OpenSSH, dan stabil antar koneksi. Jalankan: cargo test -- --ignored
    #[test]
    #[ignore]
    fn fingerprint_dari_server_nyata() {
        use crate::ssh::connect_tcp;

        let fp_of = || {
            let tcp = connect_tcp("127.0.0.1", 2222).expect("mock sshd belum jalan?");
            let mut sess = ssh2::Session::new().unwrap();
            sess.set_tcp_stream(tcp);
            sess.handshake().unwrap();
            super::fingerprint_of(&sess).unwrap()
        };

        let (key_type, fp) = fp_of();
        assert!(fp.starts_with("SHA256:"), "format OpenSSH: {}", fp);
        // 32 byte SHA256 → 43 karakter base64 tanpa padding
        assert_eq!(fp.len(), "SHA256:".len() + 43, "{}", fp);
        assert!(!key_type.is_empty());

        let (_, fp2) = fp_of();
        assert_eq!(fp, fp2, "fingerprint harus stabil antar koneksi");
    }
}
