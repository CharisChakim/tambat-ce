use serde::Serialize;
use std::fs;
use std::path::PathBuf;

/// Satu entri Host dari ~/.ssh/config yang bisa diimpor jadi host Tambat.
/// Belum punya id — id dibuat frontend saat pengguna memilih mengimpornya.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigHost {
    /// nama pola Host, dipakai sebagai label
    pub label: String,
    /// HostName bila ada, kalau tidak nama Host itu sendiri
    pub host: String,
    pub port: u16,
    /// User bila ada; string kosong = pengguna harus mengisinya sendiri
    pub username: String,
    /// "key" bila ada IdentityFile, kalau tidak "agent"
    pub auth_type: String,
    pub key_path: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".ssh").join("config"))
}

/// Pisahkan "Keyword Value" ala ssh_config: pemisahnya spasi ATAU "=", dan
/// keyword tidak peka huruf besar-kecil.
fn split_directive(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (k, v) = match line.split_once(['=', ' ', '\t']) {
        Some((k, v)) => (k, v),
        None => return None,
    };
    let v = v.trim().trim_start_matches('=').trim();
    if v.is_empty() {
        return None;
    }
    Some((k.trim().to_lowercase(), v.to_string()))
}

/// Entri yang tidak bisa dipakai sebagai host konkret: pola wildcard seperti
/// `Host *` atau `Host *.internal` hanya berisi setelan default, bukan server.
fn is_pattern(name: &str) -> bool {
    name.contains('*') || name.contains('?') || name.contains('!')
}

/// Parse isi ssh_config jadi daftar host konkret.
///
/// Sengaja minimalis: `Include` dan `Match` tidak diikuti, dan hanya
/// HostName/User/Port/IdentityFile yang dibaca — sisanya (ProxyJump, dsb.)
/// belum punya padanan di model host Tambat.
pub fn parse(text: &str) -> Vec<ConfigHost> {
    let mut out: Vec<ConfigHost> = Vec::new();
    // nama Host yang sedang dikumpulkan; satu baris Host bisa memuat beberapa alias
    let mut current: Vec<String> = Vec::new();
    let mut host_name: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut key: Option<String> = None;

    let flush = |names: &mut Vec<String>,
                     host_name: &mut Option<String>,
                     user: &mut Option<String>,
                     port: &mut Option<u16>,
                     key: &mut Option<String>,
                     out: &mut Vec<ConfigHost>| {
        for name in names.drain(..) {
            out.push(ConfigHost {
                label: name.clone(),
                host: host_name.clone().unwrap_or(name),
                port: port.unwrap_or(22),
                username: user.clone().unwrap_or_default(),
                auth_type: if key.is_some() { "key" } else { "agent" }.to_string(),
                key_path: key.clone(),
            });
        }
        *host_name = None;
        *user = None;
        *port = None;
        *key = None;
    };

    for line in text.lines() {
        let Some((keyword, value)) = split_directive(line) else {
            continue;
        };
        match keyword.as_str() {
            "host" => {
                flush(
                    &mut current,
                    &mut host_name,
                    &mut user,
                    &mut port,
                    &mut key,
                    &mut out,
                );
                current = value
                    .split_whitespace()
                    .filter(|n| !is_pattern(n))
                    .map(|n| n.to_string())
                    .collect();
            }
            "hostname" => host_name = Some(value),
            "user" => user = Some(value),
            "port" => port = value.parse().ok(),
            "identityfile" => key = Some(value),
            _ => {}
        }
    }
    flush(
        &mut current,
        &mut host_name,
        &mut user,
        &mut port,
        &mut key,
        &mut out,
    );
    out
}

/// Baca ~/.ssh/config dan kembalikan host yang bisa diimpor. Berkas tidak ada
/// bukan galat — cukup daftar kosong.
#[tauri::command]
pub fn sshconfig_hosts() -> Result<Vec<ConfigHost>, String> {
    let Some(path) = config_path() else {
        return Ok(vec![]);
    };
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("Gagal membaca {}: {}", path.display(), e))?;
    let mut hosts = parse(&text);
    // Tanpa direktif `User`, OpenSSH memakai nama pengguna lokal — ikuti itu
    // supaya hasil impor langsung bisa dipakai tanpa disunting.
    if let Ok(local) = std::env::var("USER") {
        for h in hosts.iter_mut().filter(|h| h.username.is_empty()) {
            h.username = local.clone();
        }
    }
    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn baca_entri_lengkap() {
        let cfg = "\
Host web-prod
  HostName 203.0.113.10
  User deploy
  Port 2222
  IdentityFile ~/.ssh/id_prod
";
        let h = parse(cfg);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].label, "web-prod");
        assert_eq!(h[0].host, "203.0.113.10");
        assert_eq!(h[0].username, "deploy");
        assert_eq!(h[0].port, 2222);
        assert_eq!(h[0].auth_type, "key");
        assert_eq!(h[0].key_path.as_deref(), Some("~/.ssh/id_prod"));
    }

    #[test]
    fn tanpa_hostname_pakai_nama_host() {
        let h = parse("Host example.com\n  User root\n");
        assert_eq!(h[0].host, "example.com");
        assert_eq!(h[0].port, 22, "port default 22");
        assert_eq!(h[0].auth_type, "agent", "tanpa IdentityFile → agent");
        assert_eq!(h[0].key_path, None);
    }

    #[test]
    fn pola_wildcard_dilewati() {
        let cfg = "\
Host *
  User default-user
Host bastion
  HostName 198.51.100.7
Host *.internal
  User internal
";
        let h = parse(cfg);
        let labels: Vec<&str> = h.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, vec!["bastion"], "hanya host konkret yang diambil");
    }

    #[test]
    fn setelan_tidak_bocor_antar_blok() {
        let cfg = "\
Host satu
  User alice
  Port 2200
  IdentityFile ~/.ssh/a
Host dua
  HostName 10.0.0.2
";
        let h = parse(cfg);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].label, "dua");
        assert_eq!(h[1].username, "", "User dari blok sebelumnya tak boleh terbawa");
        assert_eq!(h[1].port, 22, "Port dari blok sebelumnya tak boleh terbawa");
        assert_eq!(h[1].auth_type, "agent", "IdentityFile tak boleh terbawa");
    }

    #[test]
    fn satu_baris_host_beberapa_alias() {
        let h = parse("Host kerja kerja-vpn\n  HostName 10.1.2.3\n  User bob\n");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].label, "kerja");
        assert_eq!(h[1].label, "kerja-vpn");
        assert!(h.iter().all(|x| x.host == "10.1.2.3" && x.username == "bob"));
    }

    #[test]
    fn keyword_tak_peka_huruf_dan_boleh_pakai_sama_dengan() {
        let h = parse("HOST srv\n  hostname=192.0.2.5\n  USER=carol\n  Port = 2022\n");
        assert_eq!(h[0].host, "192.0.2.5");
        assert_eq!(h[0].username, "carol");
        assert_eq!(h[0].port, 2022);
    }

    #[test]
    fn komentar_dan_baris_kosong_diabaikan() {
        let h = parse("# catatan\n\nHost srv\n\n  # dalam blok\n  User dave\n");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].username, "dave");
    }

    #[test]
    fn config_kosong_bukan_galat() {
        assert!(parse("").is_empty());
        assert!(parse("# cuma komentar\n").is_empty());
    }
}
