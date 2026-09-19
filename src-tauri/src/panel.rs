use serde::Serialize;
use ssh2::{Session, Sftp};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, State};

use crate::ssh::{open_session, ConnectError, ConnectParams};

/// Sesi SSH kedua per tab, khusus untuk file browser (SFTP) dan statistik
/// server, agar tidak mengganggu aliran data terminal.
#[derive(Default)]
pub struct PanelState {
    pub conns: Mutex<HashMap<String, Sender<PanelCmd>>>,
}

pub enum PanelCmd {
    List {
        path: String,
        reply: Sender<Result<DirListing, String>>,
    },
    Stats {
        reply: Sender<Result<ServerStats, String>>,
    },
    /// Unduh file remote ke folder temp lokal, balas path lokalnya.
    Fetch {
        path: String,
        reply: Sender<Result<std::path::PathBuf, String>>,
    },
    /// Salin/pindahkan (cp -a / mv) src ke dalam folder dest_dir di server.
    Transfer {
        src: String,
        dest_dir: String,
        mv: bool,
        reply: Sender<Result<(), String>>,
    },
    Mkdir {
        dir: String,
        name: String,
        reply: Sender<Result<(), String>>,
    },
    Rename {
        src: String,
        new_name: String,
        reply: Sender<Result<(), String>>,
    },
    /// Hapus file/folder (rm -rf) di server.
    Delete {
        path: String,
        reply: Sender<Result<(), String>>,
    },
    /// Unduh file remote ke folder Unduhan lokal, balas path lokalnya.
    Download {
        path: String,
        reply: Sender<Result<std::path::PathBuf, String>>,
    },
    /// Unggah file lokal ke dalam folder dest_dir di server.
    Upload {
        local_path: std::path::PathBuf,
        dest_dir: String,
        reply: Sender<Result<(), String>>,
    },
    Close,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Waktu modifikasi terakhir, detik sejak epoch; None jika server tidak melaporkannya.
    pub modified: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub entries: Vec<DirEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub mount: String,
    pub total_kb: u64,
    pub used_kb: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub capacity: u8,
    pub status: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub disks: Vec<DiskUsage>,
    pub battery: Option<Battery>,
    /// rata-rata rtt ke 1.1.1.1 (Cloudflare), ms; None = timeout / ping tak tersedia
    pub ping_cf_ms: Option<f64>,
    /// rata-rata rtt ke 8.8.8.8 (Google), ms
    pub ping_google_ms: Option<f64>,
    /// suhu CPU/SoC dalam °C; None jika sensor tidak tersedia
    pub temp_c: Option<f64>,
    /// lama server menyala, detik; 0 = tidak terbaca
    pub uptime_s: u64,
    /// load average 1 menit
    pub load1: Option<f64>,
}

/// Skrip dikirim lewat stdin `sh` (bukan argumen exec) supaya tetap POSIX
/// walau login shell pengguna fish/csh. Kedua ping berjalan paralel agar
/// total tunggu maksimal ~1 detik.
const STATS_CMD: &str = r#"
echo ===MEM
grep -E '^(MemTotal|MemAvailable):' /proc/meminfo 2>/dev/null
echo ===DISK
df -kP 2>/dev/null | awk 'NR>1 && $1 ~ "^/dev/" && !seen[$1]++ {print $2" "$3" "$6}'
echo ===BAT
for b in /sys/class/power_supply/*; do
  if [ "$(cat "$b/type" 2>/dev/null)" = "Battery" ]; then
    echo "$(cat "$b/capacity" 2>/dev/null) $(cat "$b/status" 2>/dev/null)"
    break
  fi
done
echo ===TEMP
for z in /sys/class/thermal/thermal_zone*; do
  [ -r "$z/temp" ] && echo "$(cat "$z/type" 2>/dev/null) $(cat "$z/temp" 2>/dev/null)"
done
for h in /sys/class/hwmon/hwmon*; do
  [ -r "$h/temp1_input" ] && echo "$(cat "$h/name" 2>/dev/null) $(cat "$h/temp1_input" 2>/dev/null)"
done
echo ===UP
cat /proc/uptime 2>/dev/null
cat /proc/loadavg 2>/dev/null
echo ===PING
(ping -n -c 1 -W 1 1.1.1.1 2>/dev/null | sed -n 's/.*time=\([0-9.]*\).*/CF \1/p') &
(ping -n -c 1 -W 1 8.8.8.8 2>/dev/null | sed -n 's/.*time=\([0-9.]*\).*/GG \1/p') &
wait
"#;

fn parse_stats(out: &str) -> ServerStats {
    let mut s = ServerStats::default();
    let mut section = "";
    // (nama sensor huruf kecil, °C) — dipilih setelah semua baris terbaca
    let mut temps: Vec<(String, f64)> = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("===") {
            section = rest;
            continue;
        }
        match section {
            "MEM" => {
                let mut it = line.split_whitespace();
                match (it.next(), it.next()) {
                    (Some("MemTotal:"), Some(v)) => s.mem_total_kb = v.parse().unwrap_or(0),
                    (Some("MemAvailable:"), Some(v)) => s.mem_avail_kb = v.parse().unwrap_or(0),
                    _ => {}
                }
            }
            "DISK" => {
                let p: Vec<&str> = line.splitn(3, ' ').collect();
                if p.len() == 3 {
                    if let (Ok(t), Ok(u)) = (p[0].parse(), p[1].parse()) {
                        s.disks.push(DiskUsage {
                            mount: p[2].to_string(),
                            total_kb: t,
                            used_kb: u,
                        });
                    }
                }
            }
            "BAT" => {
                let mut it = line.split_whitespace();
                if let Some(c) = it.next().and_then(|c| c.parse().ok()) {
                    s.battery = Some(Battery {
                        capacity: c,
                        status: it.next().unwrap_or("").to_string(),
                    });
                }
            }
            "TEMP" => {
                // "<nama> <nilai>"; nilai umumnya miliderajat
                let mut it = line.split_whitespace();
                if let (Some(name), Some(v)) =
                    (it.next(), it.next().and_then(|v| v.parse::<f64>().ok()))
                {
                    let c = if v > 200.0 { v / 1000.0 } else { v };
                    if c > 0.0 && c < 150.0 {
                        temps.push((name.to_lowercase(), c));
                    }
                }
            }
            "UP" => {
                // /proc/uptime: "12345.67 8910.11"; /proc/loadavg: "0.84 0.71 0.66 2/401 1234"
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 4 {
                    if let Ok(l) = fields[0].parse() {
                        s.load1 = Some(l);
                    }
                } else if s.uptime_s == 0 {
                    if let Some(Ok(up)) = fields.first().map(|v| v.parse::<f64>()) {
                        s.uptime_s = up as u64;
                    }
                }
            }
            "PING" => {
                let mut it = line.split_whitespace();
                match (it.next(), it.next().and_then(|v| v.parse::<f64>().ok())) {
                    (Some("CF"), Some(ms)) => s.ping_cf_ms = Some(ms),
                    (Some("GG"), Some(ms)) => s.ping_google_ms = Some(ms),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Utamakan sensor CPU/paket; kalau tidak ada, ambil yang tertinggi.
    const PRIORITAS: [&str; 6] = ["pkg", "cpu", "core", "k10temp", "soc", "tctl"];
    s.temp_c = temps
        .iter()
        .filter(|(n, _)| PRIORITAS.iter().any(|p| n.contains(p)))
        .map(|(_, c)| *c)
        .fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c))))
        .or_else(|| {
            temps
                .iter()
                .map(|(_, c)| *c)
                .fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c))))
        });
    s
}

fn do_stats(sess: &Session) -> Result<ServerStats, String> {
    let mut ch = sess
        .channel_session()
        .map_err(|e| format!("Gagal membuka channel statistik: {}", e))?;
    ch.exec("sh").map_err(|e| format!("Gagal menjalankan sh: {}", e))?;
    ch.write_all(STATS_CMD.as_bytes())
        .map_err(|e| format!("Gagal mengirim skrip statistik: {}", e))?;
    ch.send_eof().map_err(|e| e.to_string())?;
    let mut out = String::new();
    ch.read_to_string(&mut out)
        .map_err(|e| format!("Gagal membaca statistik: {}", e))?;
    let _ = ch.close();
    let _ = ch.wait_close();
    Ok(parse_stats(&out))
}

fn do_list(sftp: &Sftp, path: &str) -> Result<DirListing, String> {
    let real = sftp
        .realpath(Path::new(path))
        .map_err(|e| format!("Path tidak valid: {}", e))?;
    let raw = sftp
        .readdir(&real)
        .map_err(|e| format!("Gagal membaca folder: {}", e))?;
    let mut entries: Vec<DirEntry> = raw
        .into_iter()
        .filter_map(|(p, st)| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let mut is_dir = st.is_dir();
            // Symlink dilaporkan lstat; resolve agar symlink ke folder bisa dibuka.
            if st.file_type().is_symlink() {
                if let Ok(target) = sftp.stat(&p) {
                    is_dir = target.is_dir();
                }
            }
            Some(DirEntry {
                name,
                is_dir,
                size: st.size.unwrap_or(0),
                modified: st.mtime,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(DirListing {
        path: real.to_string_lossy().into_owned(),
        entries,
    })
}

fn do_fetch(sftp: &Sftp, path: &str) -> Result<std::path::PathBuf, String> {
    let st = sftp
        .stat(Path::new(path))
        .map_err(|e| format!("Gagal membaca file: {}", e))?;
    if st.size.unwrap_or(0) > 200 * 1024 * 1024 {
        return Err("File terlalu besar untuk dibuka (>200 MB)".into());
    }
    let name = Path::new(path)
        .file_name()
        .ok_or("Nama file tidak valid")?
        .to_string_lossy()
        .into_owned();
    let dir = std::env::temp_dir()
        .join("tambat-open")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let local = dir.join(&name);
    let mut remote = sftp
        .open(Path::new(path))
        .map_err(|e| format!("Gagal membuka file remote: {}", e))?;
    let mut file = std::fs::File::create(&local).map_err(|e| e.to_string())?;
    std::io::copy(&mut remote, &mut file).map_err(|e| format!("Gagal mengunduh file: {}", e))?;
    Ok(local)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Jalankan perintah lewat `sh` di server; Err berisi stderr bila gagal.
fn run_sh(sess: &Session, cmd: &str, label: &str) -> Result<(), String> {
    let mut ch = sess.channel_session().map_err(|e| e.to_string())?;
    ch.exec("sh").map_err(|e| e.to_string())?;
    ch.write_all(cmd.as_bytes()).map_err(|e| e.to_string())?;
    ch.send_eof().map_err(|e| e.to_string())?;
    let mut out = String::new();
    let _ = ch.read_to_string(&mut out);
    let mut err = String::new();
    let _ = ch.stderr().read_to_string(&mut err);
    let _ = ch.close();
    let _ = ch.wait_close();
    let code = ch.exit_status().unwrap_or(-1);
    if code != 0 {
        return Err(if err.trim().is_empty() {
            format!("Perintah {} gagal (kode {})", label, code)
        } else {
            err.trim().to_string()
        });
    }
    Ok(())
}

fn do_transfer(
    sess: &Session,
    sftp: &Sftp,
    src: &str,
    dest_dir: &str,
    mv: bool,
) -> Result<(), String> {
    let name = Path::new(src)
        .file_name()
        .ok_or("Nama sumber tidak valid")?
        .to_string_lossy()
        .into_owned();
    let dest = if dest_dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dest_dir, name)
    };
    if src == dest {
        return Err("Sumber dan tujuan sama".into());
    }
    if sftp.stat(Path::new(&dest)).is_ok() {
        return Err(format!("Sudah ada \"{}\" di folder tujuan", name));
    }
    let cmd = format!(
        "{} -- {} {}\n",
        if mv { "mv" } else { "cp -a" },
        sh_quote(src),
        sh_quote(&dest)
    );
    run_sh(sess, &cmd, if mv { "mv" } else { "cp" })
}

fn do_mkdir(sftp: &Sftp, dir: &str, name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('/') {
        return Err("Nama folder tidak valid".into());
    }
    let path = if dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dir, name)
    };
    sftp.mkdir(Path::new(&path), 0o755)
        .map_err(|e| format!("Gagal membuat folder: {}", e))
}

fn do_rename(sftp: &Sftp, src: &str, new_name: &str) -> Result<(), String> {
    if new_name.is_empty() || new_name.contains('/') {
        return Err("Nama baru tidak valid".into());
    }
    let parent = Path::new(src).parent().ok_or("Path sumber tidak valid")?;
    let dest = parent.join(new_name);
    if sftp.stat(&dest).is_ok() {
        return Err(format!("Sudah ada \"{}\" di folder ini", new_name));
    }
    sftp.rename(Path::new(src), &dest, None)
        .map_err(|e| format!("Gagal mengganti nama: {}", e))
}

fn do_delete(sess: &Session, path: &str) -> Result<(), String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err("Menolak menghapus path ini".into());
    }
    run_sh(sess, &format!("rm -rf -- {}\n", sh_quote(path)), "rm")
}

/// Folder Unduhan pengguna: xdg-user-dir bila ada, fallback ~/Downloads.
fn download_dir() -> std::path::PathBuf {
    use std::process::Command;
    if let Ok(out) = Command::new("xdg-user-dir").arg("DOWNLOAD").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() && p != "/" {
            return std::path::PathBuf::from(p);
        }
    }
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
        .join("Downloads")
}

/// Hindari menimpa: "laporan.txt" yang sudah ada → "laporan (1).txt", dst.
fn unique_local(dir: &Path, name: &str) -> std::path::PathBuf {
    let mut cand = dir.join(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
        _ => (name.to_string(), String::new()),
    };
    let mut i = 1;
    while cand.exists() {
        cand = dir.join(format!("{} ({}){}", stem, i, ext));
        i += 1;
    }
    cand
}

fn do_download(sftp: &Sftp, path: &str) -> Result<std::path::PathBuf, String> {
    let name = Path::new(path)
        .file_name()
        .ok_or("Nama file tidak valid")?
        .to_string_lossy()
        .into_owned();
    let dir = download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let local = unique_local(&dir, &name);
    let mut remote = sftp
        .open(Path::new(path))
        .map_err(|e| format!("Gagal membuka file remote: {}", e))?;
    let mut file = std::fs::File::create(&local).map_err(|e| e.to_string())?;
    std::io::copy(&mut remote, &mut file).map_err(|e| format!("Gagal mengunduh file: {}", e))?;
    Ok(local)
}

fn do_upload(sftp: &Sftp, local_path: &Path, dest_dir: &str) -> Result<(), String> {
    let name = local_path
        .file_name()
        .ok_or("Nama file lokal tidak valid")?
        .to_string_lossy()
        .into_owned();
    let dest = if dest_dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dest_dir, name)
    };
    if sftp.stat(Path::new(&dest)).is_ok() {
        return Err(format!("Sudah ada \"{}\" di folder tujuan", name));
    }
    // Folder → salin rekursif; file → salin langsung. Tanpa ini, men-drag folder
    // membuat `File::open` pada direktori gagal ("Gagal membuka file lokal").
    let meta = std::fs::metadata(local_path)
        .map_err(|e| format!("Gagal membaca \"{}\": {}", name, e))?;
    if meta.is_dir() {
        upload_dir(sftp, local_path, &dest)
    } else {
        upload_file(sftp, local_path, &dest)
    }
}

/// Salin satu file lokal ke path remote `dest`. Pemanggil sudah memastikan
/// `dest` belum ada (untuk item level atas) atau berada di folder yang baru dibuat.
fn upload_file(sftp: &Sftp, local_path: &Path, dest: &str) -> Result<(), String> {
    let mut file = std::fs::File::open(local_path)
        .map_err(|e| format!("Gagal membuka file lokal: {}", e))?;
    let mut remote = sftp
        .create(Path::new(dest))
        .map_err(|e| format!("Gagal membuat file di server: {}", e))?;
    std::io::copy(&mut file, &mut remote).map_err(|e| format!("Gagal mengunggah file: {}", e))?;
    Ok(())
}

/// Unggah folder lokal secara rekursif ke `dest` (yang belum ada di server).
fn upload_dir(sftp: &Sftp, local_dir: &Path, dest: &str) -> Result<(), String> {
    sftp.mkdir(Path::new(dest), 0o755)
        .map_err(|e| format!("Gagal membuat folder di server: {}", e))?;
    let entries =
        std::fs::read_dir(local_dir).map_err(|e| format!("Gagal membaca folder lokal: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let child = entry.path();
        let cname = entry.file_name().to_string_lossy().into_owned();
        let cdest = format!("{}/{}", dest, cname);
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            upload_dir(sftp, &child, &cdest)?;
        } else {
            upload_file(sftp, &child, &cdest)?;
        }
    }
    Ok(())
}

/// Buka file lokal dengan aplikasi default pengguna; kalau tidak ada
/// asosiasi (atau `force_text`), jatuh ke text editor.
fn open_local(path: &Path, force_text: bool) -> Result<(), String> {
    use std::process::Command;
    if !force_text {
        if let Ok(st) = Command::new("xdg-open").arg(path).status() {
            if st.success() {
                return Ok(());
            }
        }
    }
    // Tidak ada asosiasi MIME: pakai handler text/plain milik pengguna.
    if let Ok(out) = Command::new("xdg-mime")
        .args(["query", "default", "text/plain"])
        .output()
    {
        let desktop = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !desktop.is_empty() {
            if let Ok(st) = Command::new("gtk-launch").arg(&desktop).arg(path).status() {
                if st.success() {
                    return Ok(());
                }
            }
        }
    }
    for editor in ["gnome-text-editor", "gedit", "kate", "mousepad", "xed"] {
        if Command::new(editor).arg(path).spawn().is_ok() {
            return Ok(());
        }
    }
    Err("Tidak menemukan aplikasi untuk membuka file ini".into())
}

/// Buka sesi panel: koneksi + auth di thread pool, lalu thread worker
/// memproses perintah List/Stats secara berurutan pada sesi blocking.
#[tauri::command]
pub async fn panel_open(
    app: AppHandle,
    state: State<'_, PanelState>,
    id: String,
    params: ConnectParams,
) -> Result<(), ConnectError> {
    if id.is_empty() || state.conns.lock().unwrap().contains_key(&id) {
        return Err("Id panel tidak valid".into());
    }

    let (sess, sftp) = tauri::async_runtime::spawn_blocking(move || {
        let sess = open_session(&app, &params)?;
        // Batasi operasi blocking agar worker tidak macet selamanya.
        sess.set_timeout(15_000);
        let sftp = sess
            .sftp()
            .map_err(|e| format!("Gagal membuka SFTP: {}", e))?;
        Ok::<_, ConnectError>((sess, sftp))
    })
    .await
    .map_err(|e| e.to_string())??;

    let (tx, rx) = channel::<PanelCmd>();
    state.conns.lock().unwrap().insert(id, tx);

    std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                PanelCmd::List { path, reply } => {
                    let _ = reply.send(do_list(&sftp, &path));
                }
                PanelCmd::Stats { reply } => {
                    let _ = reply.send(do_stats(&sess));
                }
                PanelCmd::Fetch { path, reply } => {
                    let _ = reply.send(do_fetch(&sftp, &path));
                }
                PanelCmd::Transfer {
                    src,
                    dest_dir,
                    mv,
                    reply,
                } => {
                    let _ = reply.send(do_transfer(&sess, &sftp, &src, &dest_dir, mv));
                }
                PanelCmd::Mkdir { dir, name, reply } => {
                    let _ = reply.send(do_mkdir(&sftp, &dir, &name));
                }
                PanelCmd::Rename {
                    src,
                    new_name,
                    reply,
                } => {
                    let _ = reply.send(do_rename(&sftp, &src, &new_name));
                }
                PanelCmd::Delete { path, reply } => {
                    let _ = reply.send(do_delete(&sess, &path));
                }
                PanelCmd::Download { path, reply } => {
                    let _ = reply.send(do_download(&sftp, &path));
                }
                PanelCmd::Upload {
                    local_path,
                    dest_dir,
                    reply,
                } => {
                    let _ = reply.send(do_upload(&sftp, &local_path, &dest_dir));
                }
                PanelCmd::Close => break,
            }
        }
        drop(sftp);
        let _ = sess.disconnect(None, "panel ditutup", None);
    });

    Ok(())
}

/// Kirim satu perintah ke worker panel dan tunggu balasannya; menyeragamkan
/// pola lock→ambil sender→spawn_blocking→kirim→timeout yang dipakai semua
/// command panel_* di bawah.
async fn dispatch<T, F>(
    state: &State<'_, PanelState>,
    id: &str,
    timeout: Duration,
    timeout_msg: &'static str,
    make_cmd: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Sender<Result<T, String>>) -> PanelCmd + Send + 'static,
{
    let tx = state
        .conns
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or("Panel tidak tersambung")?;
    tauri::async_runtime::spawn_blocking(move || {
        let (rtx, rrx) = channel();
        tx.send(make_cmd(rtx))
            .map_err(|_| "Panel sudah ditutup".to_string())?;
        rrx.recv_timeout(timeout).map_err(|_| timeout_msg.to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn panel_list(
    state: State<'_, PanelState>,
    id: String,
    path: String,
) -> Result<DirListing, String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(30),
        "Waktu habis membaca folder",
        |reply: Sender<Result<DirListing, String>>| PanelCmd::List { path, reply },
    )
    .await
}

#[tauri::command]
pub async fn panel_stats(state: State<'_, PanelState>, id: String) -> Result<ServerStats, String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(20),
        "Waktu habis membaca statistik",
        |reply: Sender<Result<ServerStats, String>>| PanelCmd::Stats { reply },
    )
    .await
}

/// Unduh file remote lalu buka dengan aplikasi default perangkat pengguna;
/// `text_editor` memaksa dibuka dengan text editor.
#[tauri::command]
pub async fn panel_open_file(
    state: State<'_, PanelState>,
    id: String,
    path: String,
    text_editor: bool,
) -> Result<(), String> {
    let local = dispatch(
        &state,
        &id,
        Duration::from_secs(120),
        "Waktu habis mengunduh file",
        |reply: Sender<Result<std::path::PathBuf, String>>| PanelCmd::Fetch { path, reply },
    )
    .await?;
    tauri::async_runtime::spawn_blocking(move || open_local(&local, text_editor))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn panel_transfer(
    state: State<'_, PanelState>,
    id: String,
    src: String,
    dest_dir: String,
    mv: bool,
) -> Result<(), String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(300),
        "Waktu habis menyalin/memindahkan",
        move |reply: Sender<Result<(), String>>| PanelCmd::Transfer {
            src,
            dest_dir,
            mv,
            reply,
        },
    )
    .await
}

#[tauri::command]
pub async fn panel_mkdir(
    state: State<'_, PanelState>,
    id: String,
    dir: String,
    name: String,
) -> Result<(), String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(30),
        "Waktu habis membuat folder",
        |reply: Sender<Result<(), String>>| PanelCmd::Mkdir { dir, name, reply },
    )
    .await
}

#[tauri::command]
pub async fn panel_rename(
    state: State<'_, PanelState>,
    id: String,
    src: String,
    new_name: String,
) -> Result<(), String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(30),
        "Waktu habis mengganti nama",
        |reply: Sender<Result<(), String>>| PanelCmd::Rename {
            src,
            new_name,
            reply,
        },
    )
    .await
}

#[tauri::command]
pub async fn panel_delete(
    state: State<'_, PanelState>,
    id: String,
    path: String,
) -> Result<(), String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(120),
        "Waktu habis menghapus",
        |reply: Sender<Result<(), String>>| PanelCmd::Delete { path, reply },
    )
    .await
}

/// Unduh file remote ke folder Unduhan; balas path lokal hasil unduhan.
#[tauri::command]
pub async fn panel_download(
    state: State<'_, PanelState>,
    id: String,
    path: String,
) -> Result<String, String> {
    let local = dispatch(
        &state,
        &id,
        Duration::from_secs(600),
        "Waktu habis mengunduh file",
        |reply: Sender<Result<std::path::PathBuf, String>>| PanelCmd::Download { path, reply },
    )
    .await?;
    Ok(local.to_string_lossy().into_owned())
}

/// Unggah file lokal ke folder `dest_dir` di server.
#[tauri::command]
pub async fn panel_upload(
    state: State<'_, PanelState>,
    id: String,
    local_path: std::path::PathBuf,
    dest_dir: String,
) -> Result<(), String> {
    dispatch(
        &state,
        &id,
        Duration::from_secs(600),
        "Waktu habis mengunggah file",
        move |reply: Sender<Result<(), String>>| PanelCmd::Upload {
            local_path,
            dest_dir,
            reply,
        },
    )
    .await
}

#[tauri::command]
pub fn panel_close(state: State<'_, PanelState>, id: String) {
    if let Some(tx) = state.conns.lock().unwrap().remove(&id) {
        let _ = tx.send(PanelCmd::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_stats;

    #[test]
    fn parse_stats_lengkap() {
        let out = "\n===MEM\nMemTotal:       16334996 kB\nMemAvailable:   9541234 kB\n===DISK\n102400 51200 /\n512000 128000 /data\n===BAT\n87 Charging\n===TEMP\nacpitz 27800\nx86_pkg_temp 54000\nnvme 61000\n===UP\n93784.55 180000.12\n0.84 0.71 0.66 2/401 12345\n===PING\nGG 15.7\nCF 12.34\n";
        let s = parse_stats(out);
        assert_eq!(s.uptime_s, 93784);
        assert_eq!(s.load1, Some(0.84));
        // x86_pkg_temp menang atas nvme (61°C) karena prioritas sensor CPU
        assert_eq!(s.temp_c, Some(54.0));
        assert_eq!(s.mem_total_kb, 16334996);
        assert_eq!(s.mem_avail_kb, 9541234);
        assert_eq!(s.disks.len(), 2);
        assert_eq!(s.disks[1].mount, "/data");
        assert_eq!(s.disks[1].used_kb, 128000);
        let bat = s.battery.expect("baterai terbaca");
        assert_eq!(bat.capacity, 87);
        assert_eq!(bat.status, "Charging");
        assert_eq!(s.ping_cf_ms, Some(12.34));
        assert_eq!(s.ping_google_ms, Some(15.7));
    }

    /// E2E terhadap server tiruan paramiko (tests/mock_sshd.py) di 127.0.0.1:2222.
    /// Jalankan mock-nya dulu (python3 tests/mock_sshd.py), lalu: cargo test -- --ignored
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn e2e_panel_lokal() {
        use crate::ssh::{auth, connect_tcp, ConnectParams};

        let dir = std::path::Path::new("/tmp/tambat-e2e-panel");
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join("berkas.txt"), b"halo").unwrap();
        std::os::unix::fs::symlink(dir.join("subdir"), dir.join("tautan")).unwrap();

        let tcp = connect_tcp("127.0.0.1", 2222).expect("mock sshd belum jalan?");
        let mut sess = ssh2::Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake().unwrap();
        let params = ConnectParams {
            host: "127.0.0.1".into(),
            port: 2222,
            username: "demo".into(),
            auth_type: "password".into(),
            password: Some("demo".into()),
            key_path: None,
            key_passphrase: None,
            cols: 80,
            rows: 24,
        };
        auth(&sess, &params).unwrap();
        sess.set_timeout(15_000);
        let sftp = sess.sftp().unwrap();

        let l = super::do_list(&sftp, "/tmp/tambat-e2e-panel").unwrap();
        assert_eq!(l.path, "/tmp/tambat-e2e-panel");
        let names: Vec<_> = l.entries.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        assert!(names.contains(&("subdir", true)), "{:?}", names);
        assert!(names.contains(&("berkas.txt", false)), "{:?}", names);
        assert!(
            names.contains(&("tautan", true)),
            "symlink ke folder harus dianggap folder: {:?}",
            names
        );
        assert!(l.entries[0].is_dir, "folder harus diurutkan lebih dulu");
        let berkas = l.entries.iter().find(|e| e.name == "berkas.txt").unwrap();
        assert!(berkas.modified.is_some(), "waktu modifikasi harus terbaca");

        let s = super::do_stats(&sess).unwrap();
        assert!(s.mem_total_kb > 0, "MemTotal harus terbaca");
        assert!(s.mem_avail_kb > 0, "MemAvailable harus terbaca");
        assert!(!s.disks.is_empty(), "df harus mengembalikan minimal 1 mount");

        // Unduh file (dipakai fitur "buka dengan aplikasi default")
        let local = super::do_fetch(&sftp, "/tmp/tambat-e2e-panel/berkas.txt").unwrap();
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "halo");

        // Unggah file baru dari lokal (folder terpisah dari folder remote di atas), tolak duplikat
        let upload_dir = std::path::Path::new("/tmp/tambat-e2e-upload-src");
        let _ = std::fs::remove_dir_all(upload_dir);
        std::fs::create_dir_all(upload_dir).unwrap();
        let upload_src = upload_dir.join("unggahan.txt");
        std::fs::write(&upload_src, b"data unggahan").unwrap();
        super::do_upload(&sftp, &upload_src, "/tmp/tambat-e2e-panel").unwrap();
        let uploaded = super::do_fetch(&sftp, "/tmp/tambat-e2e-panel/unggahan.txt").unwrap();
        assert_eq!(std::fs::read_to_string(&uploaded).unwrap(), "data unggahan");
        assert!(
            super::do_upload(&sftp, &upload_src, "/tmp/tambat-e2e-panel").is_err(),
            "duplikat saat unggah harus ditolak"
        );

        // Salin ke subdir, tolak duplikat, lalu pindahkan ke folder lain
        super::do_transfer(
            &sess,
            &sftp,
            "/tmp/tambat-e2e-panel/berkas.txt",
            "/tmp/tambat-e2e-panel/subdir",
            false,
        )
        .unwrap();
        assert!(dir.join("subdir/berkas.txt").exists());
        assert!(
            super::do_transfer(
                &sess,
                &sftp,
                "/tmp/tambat-e2e-panel/berkas.txt",
                "/tmp/tambat-e2e-panel/subdir",
                false,
            )
            .is_err(),
            "duplikat harus ditolak"
        );
        std::fs::create_dir_all(dir.join("dir2")).unwrap();
        super::do_transfer(
            &sess,
            &sftp,
            "/tmp/tambat-e2e-panel/subdir/berkas.txt",
            "/tmp/tambat-e2e-panel/dir2",
            true,
        )
        .unwrap();
        assert!(dir.join("dir2/berkas.txt").exists());
        assert!(!dir.join("subdir/berkas.txt").exists(), "mv harus memindahkan");

        // Folder baru, ganti nama, hapus (fitur menu klik kanan)
        super::do_mkdir(&sftp, "/tmp/tambat-e2e-panel", "folder baru").unwrap();
        assert!(dir.join("folder baru").is_dir());
        assert!(
            super::do_mkdir(&sftp, "/tmp/tambat-e2e-panel", "a/b").is_err(),
            "nama folder dengan '/' harus ditolak"
        );

        super::do_rename(&sftp, "/tmp/tambat-e2e-panel/berkas.txt", "riwayat.txt").unwrap();
        assert!(dir.join("riwayat.txt").exists());
        assert!(!dir.join("berkas.txt").exists());
        assert!(
            super::do_rename(&sftp, "/tmp/tambat-e2e-panel/riwayat.txt", "riwayat.txt").is_err(),
            "nama tujuan yang sudah ada harus ditolak"
        );

        super::do_delete(&sess, "/tmp/tambat-e2e-panel/dir2").unwrap();
        assert!(!dir.join("dir2").exists(), "rm -rf harus menghapus folder beserta isinya");
        assert!(
            super::do_delete(&sess, "relatif/path").is_err(),
            "path relatif harus ditolak"
        );
    }

    #[test]
    fn unique_local_menghindari_timpa() {
        let dir = std::env::temp_dir().join(format!("tambat-uji-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(super::unique_local(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), b"").unwrap();
        assert_eq!(super::unique_local(&dir, "a.txt"), dir.join("a (1).txt"));
        std::fs::write(dir.join("a (1).txt"), b"").unwrap();
        assert_eq!(super::unique_local(&dir, "a.txt"), dir.join("a (2).txt"));
        // file tersembunyi: titik di awal bukan pemisah ekstensi
        std::fs::write(dir.join(".bashrc"), b"").unwrap();
        assert_eq!(super::unique_local(&dir, ".bashrc"), dir.join(".bashrc (1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_stats_tanpa_baterai_dan_ping_timeout() {
        let out = "===MEM\nMemTotal: 1024 kB\nMemAvailable: 512 kB\n===DISK\n===BAT\n===TEMP\n===UP\n===PING\n";
        let s = parse_stats(out);
        assert!(s.battery.is_none());
        assert!(s.disks.is_empty());
        assert_eq!(s.ping_cf_ms, None);
        assert_eq!(s.ping_google_ms, None);
        assert_eq!(s.temp_c, None);
        assert_eq!(s.uptime_s, 0);
        assert_eq!(s.load1, None);
    }
}
