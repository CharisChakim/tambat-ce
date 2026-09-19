<p align="center">
  <img src="docs/logo.svg" alt="Logo Tambat — jangkar menambat ke server" width="150">
</p>

# Tambat Community Edition

**Rilis stabil: v1.0.0**

**SSH client desktop yang ringan, aman, dan enak dipakai** — terminal, file
manager (SFTP), dan monitor server jadi satu, berdampingan di tiap tab.
Alternatif **gratis & open-source** untuk Termius/MobaXterm.

Dibangun dengan **Tauri 2** (Rust + `ssh2`) dan **React + xterm.js** — memakai
WebView bawaan sistem (bukan Chromium tersemat), jadi binarinya hanya ~7 MB.

*Tambat* (bahasa Indonesia): menambatkan — seperti kapal yang ditambatkan ke dermaga. 🪢

![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![xterm.js](https://img.shields.io/badge/xterm.js-terminal-3A3A3A)
![License: MIT](https://img.shields.io/badge/license-MIT-green)

## Tampilan

| Sebelum connect | Sesudah connect |
| :---: | :---: |
| ![Layar welcome sebelum menambatkan sesi](docs/before-connect.png) | ![Sesi aktif: terminal + panel file setelah connect](docs/after-connect.png) |

## ⚓ Filosofi nama

Dalam pelayaran, **menambatkan** adalah mengikat kapal ke dermaga dengan tali —
menahannya tetap di tempat di tengah arus, aman dan siap dimuati. Sebuah sesi
SSH pada dasarnya sama: Anda melempar tali ke server nun jauh, mengikatnya erat,
lalu bekerja dari geladak sendiri seolah kapal dan dermaga menyatu.

Setiap **host** adalah dermaga. Setiap **tab** adalah satu tali tambat. Aksen
**amber** di seluruh aplikasi adalah *lampu dermaga* yang menuntun kapal pulang
di malam hari. Maka namanya **Tambat** — bukan sekadar "menyambung", tetapi
menautkan diri dengan mantap ke sisi seberang, selama Anda membutuhkannya.

## ✨ Kenapa Tambat?

- **Satu jendela, semua yang Anda butuh** — terminal berwarna penuh, file
  browser SFTP, dan statistik server (RAM · disk · suhu · baterai · ping)
  berdampingan di setiap tab.
- **Aman sejak awal** — password & passphrase disimpan di **keyring sistem**
  OS, tidak pernah ditulis ke file biasa.
- **Alur kerja cepat** — cari host dengan `/`, connect satu klik, sidebar
  otomatis menciut jadi rail saat sesi mulai, dan panel file mengikuti `cd`
  Anda di terminal secara live.
- **Terasa seperti desktop** — jelajah file berkolom (Nama/Ukuran/Dimodifikasi),
  dobel-klik untuk buka, dan drag-and-drop file dari OS untuk mengunggah.

## Fitur

**Koneksi & terminal**
- Session manager: simpan host (label, alamat, port, user), cari kilat dengan `/`
- Autentikasi password, private key (+passphrase), dan SSH agent
- Terminal penuh via xterm.js (xterm-256color, scrollback 8000 baris, klik URL)
- Multi-tab dengan indikator status koneksi live, tutup tab `Ctrl+Shift+W`
- Sidebar host bisa menciut jadi rail sempit — otomatis saat sesi pertama,
  tetap satu klik untuk menyambung

**File & SFTP (panel per tab)**
- Jelajah folder dengan tampilan kolom Nama / Ukuran / Dimodifikasi
- Dobel-klik buka file dengan aplikasi default; salin, pindah, ganti nama,
  hapus, dan buat folder di server
- **Unggah**: tombol toolbar (pilih file) atau drag-and-drop langsung dari
  file manager OS
- **Unduh** file terpilih ke folder Unduhan
- Panel otomatis mengikuti direktori kerja terminal (via OSC 7)

**Monitor & keamanan**
- Statistik server live: RAM, disk per partisi, suhu CPU, baterai, ping ke 1.1.1.1/8.8.8.8
- Secret per host: tanya tiap kali · ingat selama app berjalan · atau simpan
  permanen di **keyring sistem** (Secret Service / Keychain / Credential Manager)
- Daftar host tersimpan sebagai JSON di direktori data app
  (Linux: `~/.local/share/app.tambat.desktop/hosts.json`)

## Prasyarat build

**Linux (Debian/Ubuntu):**

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
```

**Rust** (butuh 1.77.2+ untuk Tauri 2):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Node.js** 18+ (untuk frontend).

Untuk target Windows/macOS, lihat prasyarat resmi Tauri:
https://tauri.app/start/prerequisites/

## Menjalankan

```bash
npm install
npm run tauri dev      # mode pengembangan (hot reload)
npm run tauri build    # build semua format rilis yang didukung OS aktif
```

Untuk membuat installer NSIS di Windows saja:

```bash
npm run tauri build -- --bundles nsis
```

Build pertama mengompilasi banyak crate Rust — bisa beberapa menit. Build
berikutnya jauh lebih cepat. Format installer mengikuti sistem operasi tempat
build dijalankan: NSIS/MSI di Windows, AppImage/DEB/RPM di Linux, dan DMG/app
bundle di macOS.

## Versi dan rilis desktop

Community Edition memakai versi SemVer sendiri dan tag Git berawalan `ce-v`,
misalnya `ce-v1.0.0`. Skema ini memisahkan riwayat rilis CE dari produk utama,
yang memakai satu installer untuk akun Free dan Pro.

Workflow `.github/workflows/release-desktop.yml` membangun installer Windows
x64, Linux x64, macOS Apple Silicon, dan macOS Intel. Jalankan workflow secara
manual atau push tag yang cocok dengan versi aplikasi:

```bash
git tag ce-v1.0.0
git push origin ce-v1.0.0
```

Installer Windows dan macOS pada rilis komunitas awal belum memakai sertifikat
publisher komersial. macOS memakai ad-hoc signing sehingga pengguna masih perlu
mengizinkan aplikasi melalui Privacy & Security saat pertama dibuka.

## Arsitektur singkat

```
Frontend (React + TS)                Backend (Rust)
┌─────────────────────┐   invoke    ┌──────────────────────────┐
│ App / Sidebar / Tab │ ──────────► │ ssh_connect / send /     │
│ TermView (xterm.js) │             │ resize / disconnect      │
│                     │ ◄────────── │ hosts_list / save / del  │
└─────────────────────┘   event     └──────────────────────────┘
                       ssh-data-{id}      │ satu thread IO per koneksi
                       ssh-exit-{id}      ▼
                                    ssh2 (libssh2) → server
```

- Setiap koneksi berjalan di thread sendiri dengan session non-blocking:
  membaca output server → dikirim ke frontend sebagai event base64;
  input keyboard / resize / disconnect masuk lewat kanal `mpsc`.
- `src-tauri/src/ssh.rs` — seluruh logika SSH terminal.
- `src-tauri/src/panel.rs` — sesi SSH kedua per tab untuk file browser (SFTP)
  dan statistik server, agar tidak mengganggu aliran data terminal.
- `src-tauri/src/secrets.rs` — simpan/baca password di keyring sistem.
- `src-tauri/src/hosts.rs` — CRUD daftar host (JSON).
- `src/components/TermView.tsx` — siklus hidup terminal per tab.
- `src/components/FilePanel.tsx` — file browser + statistik server.

## Test

```bash
cd src-tauri
cargo test                    # test unit (parsing statistik, dll.)

# Test E2E butuh mock sshd (paramiko) di 127.0.0.1:2222 + Secret Service aktif:
python3 tests/mock_sshd.py &
cargo test -- --ignored
```

## Roadmap

1. **Verifikasi host key** — host key server belum diverifikasi (known_hosts).
   Tambahkan `sess.known_hosts()` + dialog konfirmasi fingerprint sebelum
   dipakai di jaringan yang tidak dipercaya.
2. Split pane, snippet/command palette, port forwarding UI, jump host,
   grup/folder host, dan tema terang.

## Lisensi

[MIT](LICENSE)
