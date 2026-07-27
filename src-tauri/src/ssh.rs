use crate::hostkeys::{self, HostKeyInfo, Verdict};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use ssh2::{KeyboardInteractivePrompt, Prompt, PtyModeOpcode, PtyModes, Session};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender, TryRecvError};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// Perintah yang dikirim dari UI ke thread IO milik satu koneksi.
pub enum SshCmd {
    Input(Vec<u8>),
    Resize(u32, u32),
    Disconnect,
}

/// State global: peta id koneksi -> kanal perintah ke thread IO-nya.
#[derive(Default)]
pub struct SshState {
    pub conns: Mutex<HashMap<String, Sender<SshCmd>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// "password" | "key" | "agent"
    pub auth_type: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
    pub cols: u32,
    pub rows: u32,
}

/// Kegagalan membuka koneksi. `HostKey` dipisahkan dari galat biasa supaya
/// frontend bisa menampilkan dialog konfirmasi fingerprint alih-alih pesan galat.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectError {
    HostKey { info: HostKeyInfo },
    Other { message: String },
}

impl From<String> for ConnectError {
    fn from(message: String) -> Self {
        ConnectError::Other { message }
    }
}

impl From<&str> for ConnectError {
    fn from(message: &str) -> Self {
        ConnectError::Other {
            message: message.to_string(),
        }
    }
}

/// Interval keepalive (detik). Tanpa ini, NAT/firewall memutus sesi yang diam
/// dan pengguna hanya melihat "Koneksi ditutup" tanpa sebab.
const KEEPALIVE_S: u32 = 30;

/// Batas waktu menyambung TCP, per alamat hasil resolve. Sengaja pendek: host
/// yang mati atau porta yang di-drop firewall harus ketahuan cepat, bukan
/// setelah pengguna menunggu belasan detik di depan layar kosong.
const TCP_CONNECT_TIMEOUT_S: u64 = 3;

/// Batas waktu handshake SSH. Tanpa ini, porta yang terbuka tapi tidak pernah
/// mengirim banner SSH (load balancer, honeypot, layanan lain) menggantung
/// selamanya — `connect_timeout` sudah lewat pada tahap ini.
const HANDSHAKE_TIMEOUT_MS: u32 = 3_000;

/// Auth diberi jatah lebih longgar daripada handshake: PAM, LDAP, dan modul 2FA
/// di server memang kadang lambat menjawab, dan itu bukan tanda server mati.
const AUTH_TIMEOUT_MS: u32 = 30_000;

/// TCP → handshake → verifikasi host key → keepalive → auth.
///
/// Verifikasi host key sengaja berada SEBELUM `auth`: kalau identitas server
/// belum terbukti, password/passphrase tidak boleh sampai terkirim.
pub(crate) fn open_session(
    app: &AppHandle,
    p: &ConnectParams,
) -> Result<Session, ConnectError> {
    let host = normalize_host(&p.host).to_string();
    let tcp = connect_tcp(&host, p.port)?;
    tcp.set_nodelay(true).ok();

    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_tcp_stream(tcp);
    sess.set_timeout(HANDSHAKE_TIMEOUT_MS);
    sess.handshake()
        .map_err(|e| format!("Handshake SSH gagal: {}", e))?;

    match hostkeys::verify(app, &sess, &host, p.port)? {
        Verdict::Trusted => {}
        Verdict::Untrusted(info) => return Err(ConnectError::HostKey { info }),
    }

    sess.set_timeout(AUTH_TIMEOUT_MS);
    sess.set_keepalive(true, KEEPALIVE_S);
    auth(&sess, p)?;
    Ok(sess)
}

/// Expand "~/..." ke direktori home agar path seperti ~/.ssh/id_ed25519 berfungsi.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Bersihkan input host: buang skema URL (http://, ssh://), path, dan spasi,
/// sehingga entri seperti "http://192.168.1.10/" tetap bisa di-resolve.
fn normalize_host(host: &str) -> &str {
    let h = host.trim();
    let h = h.split_once("://").map_or(h, |(_, rest)| rest);
    let h = h.split(['/', '?', '#']).next().unwrap_or(h);
    // IPv6 literal biasa ditulis dalam kurung siku ala URL ("[::1]"), tapi
    // to_socket_addrs mengharapkan alamat polos ("::1").
    h.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(h)
}

/// Coba semua alamat hasil resolve (IPv4/IPv6) sampai satu berhasil.
pub(crate) fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let host = normalize_host(host);
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Alamat tidak valid: {}", e))?;
    let mut last_err = format!("Alamat {} tidak dapat di-resolve", host);
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(TCP_CONNECT_TIMEOUT_S)) {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last_err = format!("Gagal terhubung ke {}: {}", addr, e),
        }
    }
    Err(last_err)
}

/// Menjawab tantangan `keyboard-interactive` dengan password yang sudah dimiliki.
///
/// Banyak server mematikan `PasswordAuthentication` tapi membiarkan
/// `KbdInteractiveAuthentication` menyala; di server seperti itu satu-satunya
/// tantangan yang diajukan adalah "Password:" — sama saja dengan auth password
/// biasa, hanya dibungkus protokol yang berbeda, jadi tidak perlu bertanya lagi
/// ke pengguna.
///
/// Tantangan yang jawabannya boleh terlihat (`echo == true`, misalnya
/// "Username:") dijawab kosong: itu bukan password, dan menebaknya justru
/// mengirimkan password ke tempat yang salah. Server yang meminta kode OTP
/// karena itu akan gagal — dukungan 2FA butuh dialog yang menampilkan teks
/// tantangan apa adanya, dan itu belum ada.
struct PasswordPrompter<'a> {
    password: &'a str,
}

impl KeyboardInteractivePrompt for PasswordPrompter<'_> {
    fn prompt(&mut self, _username: &str, _instructions: &str, prompts: &[Prompt]) -> Vec<String> {
        prompts
            .iter()
            .map(|p| {
                if p.echo {
                    String::new()
                } else {
                    self.password.to_string()
                }
            })
            .collect()
    }
}

pub(crate) fn auth(sess: &Session, p: &ConnectParams) -> Result<(), String> {
    match p.auth_type.as_str() {
        "password" => {
            let pw = p.password.as_deref().unwrap_or("");
            // Tanyakan dulu metode apa yang diterima server. Menebak lalu gagal
            // memakai satu jatah `MaxAuthTries` dan menambah hitungan fail2ban,
            // sedangkan permintaan daftar ini ("none") tidak dihitung gagal.
            let methods = sess.auth_methods(&p.username).unwrap_or("").to_string();
            // Server yang menerima auth "none" sudah lolos di titik ini.
            if sess.authenticated() {
                return Ok(());
            }
            if !methods.contains("password") && methods.contains("keyboard-interactive") {
                let mut prompter = PasswordPrompter { password: pw };
                sess.userauth_keyboard_interactive(&p.username, &mut prompter)
                    .map_err(|e| format!("Autentikasi password gagal: {}", e))?;
            } else {
                sess.userauth_password(&p.username, pw)
                    .map_err(|e| format!("Autentikasi password gagal: {}", e))?;
            }
        }
        "key" => {
            let key = expand_tilde(
                p.key_path
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or("Path private key belum diisi")?,
            );
            if !key.exists() {
                return Err(format!("File private key tidak ditemukan: {}", key.display()));
            }
            sess.userauth_pubkey_file(
                &p.username,
                None,
                &key,
                p.key_passphrase.as_deref().filter(|s| !s.is_empty()),
            )
            .map_err(|e| format!("Autentikasi key gagal: {}", e))?;
        }
        "agent" => {
            let mut agent = sess.agent().map_err(|e| e.to_string())?;
            agent
                .connect()
                .map_err(|e| format!("SSH agent tidak ditemukan: {}", e))?;
            agent.list_identities().map_err(|e| e.to_string())?;
            let ids = agent.identities().map_err(|e| e.to_string())?;
            if ids.is_empty() {
                return Err("SSH agent tidak punya identity (jalankan ssh-add)".into());
            }
            let mut ok = false;
            for id in &ids {
                if agent.userauth(&p.username, id).is_ok() {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return Err("Semua identity di agent ditolak server".into());
            }
        }
        other => return Err(format!("Metode auth tidak dikenal: {}", other)),
    }
    if !sess.authenticated() {
        return Err("Autentikasi ditolak server".into());
    }
    Ok(())
}

/// Buka koneksi SSH + shell PTY, lalu jalankan loop IO di thread terpisah.
/// `id` dibuat frontend agar listener event bisa dipasang SEBELUM data mengalir.
/// Bagian blocking (TCP/handshake/auth) berjalan di thread pool, bukan main thread.
#[tauri::command]
pub async fn ssh_connect(
    app: AppHandle,
    state: State<'_, SshState>,
    id: String,
    params: ConnectParams,
) -> Result<(), ConnectError> {
    if id.is_empty() || state.conns.lock().unwrap().contains_key(&id) {
        return Err("Id sesi tidak valid".into());
    }

    let app_conn = app.clone();
    let (sess, mut ch) = tauri::async_runtime::spawn_blocking(move || {
        let sess = open_session(&app_conn, &params)?;

        let mut ch = sess
            .channel_session()
            .map_err(|e| format!("Gagal membuka channel: {}", e))?;
        // Mulai PTY dengan ECHO mati supaya snippet integrasi shell (di bawah)
        // tidak ter-echo ke layar saat dikirim. Snippet akan menyalakan echo
        // kembali (`stty echo`) di akhir untuk input pengguna berikutnya.
        let mut modes = PtyModes::new();
        modes.set_boolean(PtyModeOpcode::ECHO, false);
        ch.request_pty(
            "xterm-256color",
            Some(modes),
            Some((params.cols, params.rows, 0, 0)),
        )
        .map_err(|e| format!("Gagal meminta PTY: {}", e))?;
        ch.shell().map_err(|e| format!("Gagal membuka shell: {}", e))?;

        // Pasang hook precmd (bash: PROMPT_COMMAND, zsh: precmd_functions) yang
        // melaporkan direktori kerja lewat OSC 7 setiap prompt digambar, supaya
        // panel file di frontend bisa mengikuti `cd` di terminal secara live.
        // Dikirim sebagai baris perintah biasa (bukan lewat channel terpisah)
        // karena ssh2 hanya memberi satu channel shell interaktif per sesi.
        // Sintaks array bash/zsh (`+=(...)`) ditaruh dalam `eval` string tunggal-kutip
        // supaya shell POSIX polos (dash/ash) tidak ikut mem-parsingnya sama sekali —
        // tanpa ini, shell login yang bukan bash/zsh gagal parse dan menampilkan error.
        // Diakhiri `stty echo` (nyalakan lagi echo utk input pengguna) lalu
        // `printf '\033[1A\033[2K'` (naik 1 baris, hapus baris prompt pertama) supaya
        // eksekusi snippet tidak menyisakan prompt duplikat — prompt kedua menimpanya.
        let osc7_setup = r#"if [ -n "$ZSH_VERSION" ]; then eval '__tambat_osc7() { printf "\033]7;file://%s%s\007" "$(hostname)" "$PWD"; }; case "${precmd_functions[*]-}" in *__tambat_osc7*) ;; *) precmd_functions+=(__tambat_osc7);; esac'; elif [ -n "$BASH_VERSION" ]; then eval '__tambat_osc7() { printf "\033]7;file://%s%s\007" "$(hostname)" "$PWD"; }; case "$PROMPT_COMMAND" in *__tambat_osc7*) ;; *) PROMPT_COMMAND="__tambat_osc7${PROMPT_COMMAND:+; $PROMPT_COMMAND}";; esac'; fi; type __tambat_osc7 >/dev/null 2>&1 && __tambat_osc7; stty echo 2>/dev/null; printf '\033[1A\033[2K'
"#;
        let _ = ch.write_all(osc7_setup.as_bytes());

        // Setelah shell siap, pindah ke mode non-blocking untuk loop IO.
        sess.set_blocking(false);
        Ok::<_, ConnectError>((sess, ch))
    })
    .await
    .map_err(|e| e.to_string())??;

    let (tx, rx) = channel::<SshCmd>();
    state.conns.lock().unwrap().insert(id.clone(), tx);

    let ev_data = format!("ssh-data-{}", id);
    let ev_exit = format!("ssh-exit-{}", id);
    let id_thread = id.clone();

    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        let mut exit_msg = String::from("Koneksi ditutup");
        'io: loop {
            let mut busy = false;

            // 1. Baca output dari server, teruskan ke frontend.
            loop {
                match ch.read(&mut buf) {
                    Ok(0) => {
                        if ch.eof() {
                            break 'io;
                        }
                        break;
                    }
                    Ok(n) => {
                        busy = true;
                        let _ = app.emit(&ev_data, B64.encode(&buf[..n]));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        exit_msg = format!("Koneksi terputus: {}", e);
                        break 'io;
                    }
                }
            }
            if ch.eof() {
                break 'io;
            }

            // 2. Proses perintah dari UI (input keyboard, resize, disconnect).
            loop {
                match rx.try_recv() {
                    Ok(SshCmd::Input(data)) => {
                        busy = true;
                        let mut off = 0;
                        while off < data.len() {
                            match ch.write(&data[off..]) {
                                Ok(n) => off += n,
                                Err(ref e)
                                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                                {
                                    // Tetap baca output server selagi menunggu,
                                    // agar tidak deadlock saat window kirim penuh.
                                    if let Ok(n) = ch.read(&mut buf) {
                                        if n > 0 {
                                            let _ = app.emit(&ev_data, B64.encode(&buf[..n]));
                                        }
                                    }
                                    std::thread::sleep(Duration::from_millis(2));
                                }
                                Err(e) => {
                                    exit_msg = format!("Gagal mengirim data: {}", e);
                                    break 'io;
                                }
                            }
                        }
                    }
                    Ok(SshCmd::Resize(c, r)) => {
                        let _ = ch.request_pty_size(c, r, None, None);
                    }
                    Ok(SshCmd::Disconnect) => break 'io,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break 'io,
                }
            }

            if !busy {
                std::thread::sleep(Duration::from_millis(8));
            }
        }

        sess.set_blocking(true);
        let _ = ch.close();
        let _ = ch.wait_close();
        let _ = app.emit(&ev_exit, exit_msg);

        if let Some(st) = app.try_state::<SshState>() {
            st.conns.lock().unwrap().remove(&id_thread);
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{connect_tcp, normalize_host, TCP_CONNECT_TIMEOUT_S};
    use std::time::{Duration, Instant};

    /// Host yang tidak menjawab harus menyerah dalam hitungan detik, bukan
    /// membuat pengguna menunggu lama di layar "menghubungkan…".
    /// 192.0.2.1 = TEST-NET-1 (RFC 5737), dijamin tidak dirutekan ke mana pun.
    #[test]
    fn host_tak_terjangkau_menyerah_cepat() {
        let t0 = Instant::now();
        assert!(connect_tcp("192.0.2.1", 22).is_err());
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(TCP_CONNECT_TIMEOUT_S + 2),
            "menyerah setelah {:?}, batasnya {}s",
            elapsed,
            TCP_CONNECT_TIMEOUT_S
        );
    }

    /// Server yang mematikan `PasswordAuthentication` tapi menyalakan
    /// `KbdInteractiveAuthentication` harus tetap bisa dimasuki dengan password
    /// yang sama, tanpa bertanya apa pun lagi ke pengguna. Sebelum dukungan ini
    /// ada, server semacam itu tertutup sama sekali.
    ///
    /// Menyalakan mock sshd sendiri di porta terpisah (2223) supaya tidak
    /// bentrok dengan mock mode password di 2222 yang dipakai E2E lain.
    #[test]
    #[ignore]
    fn auth_keyboard_interactive_saat_password_dimatikan() {
        use super::{auth, ConnectParams};
        use ssh2::Session;
        use std::net::TcpStream;
        use std::process::{Command, Stdio};

        const PORT: u16 = 2223;

        /// Bunuh mock sshd walau assert di bawahnya panik.
        struct Mock(std::process::Child);
        impl Drop for Mock {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let _mock = Mock(
            Command::new("python3")
                .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_sshd.py"))
                .env("MOCK_SSHD_PORT", PORT.to_string())
                .env("MOCK_SSHD_AUTH", "keyboard-interactive")
                .stdout(Stdio::null())
                .spawn()
                .expect("butuh python3 + paramiko"),
        );

        let tcp = (0..50)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(100));
                TcpStream::connect(("127.0.0.1", PORT)).ok()
            })
            .expect("mock sshd tidak pernah siap");

        let mut sess = Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.set_timeout(10_000);
        sess.handshake().unwrap();

        let p = ConnectParams {
            host: "127.0.0.1".into(),
            port: PORT,
            username: "demo".into(),
            auth_type: "password".into(),
            password: Some("demo".into()),
            key_path: None,
            key_passphrase: None,
            cols: 80,
            rows: 24,
        };

        assert!(
            !sess.auth_methods("demo").unwrap().contains("password"),
            "prasyarat: mock harus menolak auth password polos"
        );
        auth(&sess, &p).expect("keyboard-interactive harus dijawab dengan password");
        assert!(sess.authenticated());
    }

    #[test]
    fn normalisasi_host() {
        assert_eq!(normalize_host("http://100.96.248.104/"), "100.96.248.104");
        assert_eq!(normalize_host("ssh://example.com"), "example.com");
        assert_eq!(normalize_host("  192.168.1.10  "), "192.168.1.10");
        assert_eq!(normalize_host("example.com/path?x=1"), "example.com");
        assert_eq!(normalize_host("example.com"), "example.com");
    }

    #[test]
    fn normalisasi_ipv6_literal() {
        assert_eq!(normalize_host("[::1]"), "::1");
        assert_eq!(normalize_host("  [2001:db8::42]  "), "2001:db8::42");
        assert_eq!(normalize_host("ssh://[fe80::1]/"), "fe80::1");
        // tanpa kurung siku sudah dapat di-resolve apa adanya
        assert_eq!(normalize_host("::1"), "::1");
    }

    /// Alamat IPv6 literal harus lolos sampai ke resolusi soket, bukan ditolak
    /// dengan "Alamat tidak valid" seperti sebelum kurung sikunya dilepas.
    #[test]
    fn ipv6_literal_dapat_diresolve() {
        use std::net::ToSocketAddrs;
        assert!(
            (normalize_host("[::1]"), 22u16).to_socket_addrs().is_ok(),
            "IPv6 literal dalam kurung siku harus bisa di-resolve"
        );
        assert!(
            ("[::1]", 22u16).to_socket_addrs().is_err(),
            "prasyarat: bentuk berkurung siku memang ditolak to_socket_addrs"
        );
    }
}

#[tauri::command]
pub fn ssh_send(state: State<'_, SshState>, id: String, data_b64: String) -> Result<(), String> {
    let data = B64.decode(data_b64).map_err(|e| e.to_string())?;
    if let Some(tx) = state.conns.lock().unwrap().get(&id) {
        tx.send(SshCmd::Input(data)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn ssh_resize(state: State<'_, SshState>, id: String, cols: u32, rows: u32) {
    if let Some(tx) = state.conns.lock().unwrap().get(&id) {
        let _ = tx.send(SshCmd::Resize(cols, rows));
    }
}

#[tauri::command]
pub fn ssh_disconnect(state: State<'_, SshState>, id: String) {
    if let Some(tx) = state.conns.lock().unwrap().get(&id) {
        let _ = tx.send(SshCmd::Disconnect);
    }
}
