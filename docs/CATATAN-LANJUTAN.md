# Catatan Lanjutan

Ringkasan keadaan proyek untuk melanjutkan pekerjaan di sesi berikutnya.

**Terakhir diperbarui:** 2026-07-26
**Commit terakhir:** `9a024c2`

---

## Cara menjalankan & menguji

### Build dan pasang

```bash
npm run tauri build -- --no-bundle
cp src-tauri/target/release/tambat ~/.local/bin/tambat
rm -rf ~/.local/share/app.tambat.desktop/{WebKitCache,CacheStorage}
```

**Jangan** memakai `cargo build --release` untuk menghasilkan binary yang akan
dijalankan. Pemilihan `devUrl` vs `frontendDist` ditentukan oleh orkestrasi
`tauri-cli`, bukan profil compile — binary hasil `cargo build` akan mencoba
memuat dev server dan webview menampilkan halaman kosong bertuliskan
"Could not connect to localhost: Connection refused". Gejalanya sangat mirip
"aplikasi rusak" padahal hanya salah cara build.

Menghapus cache WebKit itu wajib setelah rebuild: tanpa itu app kadang masih
menampilkan frontend lama. Jangan hapus folder induknya — ada `hosts.json`
di situ.

### Test otomatis

```bash
npx tsc --noEmit                     # frontend
cd src-tauri && cargo test           # 17 unit test
python3 src-tauri/tests/mock_sshd.py &   # paramiko, port 2222, user demo/demo
cd src-tauri && cargo test -- --ignored --test-threads=1   # 3 E2E
```

Test keyring butuh sesi desktop dengan Secret Service aktif. Mock sshd
menghasilkan host key baru setiap kali dijalankan, jadi Tambat akan menampilkan
dialog "host key BERUBAH" setelah mock direstart — itu perilaku yang benar.

### Menguji UI tanpa menyentuh data asli

Jalankan dengan `HOME` dan `XDG_DATA_HOME` diarahkan ke folder sementara. App
akan memakai `hosts.json`, `known_hosts.json`, dan `~/.ssh/config` dari folder
itu, bukan milik pengguna:

```bash
PROF=/tmp/tambat-uji
mkdir -p "$PROF/app.tambat.desktop" "$PROF/.ssh"
HOME="$PROF" USER=namauser XDG_DATA_HOME="$PROF" ./src-tauri/target/release/tambat
```

---

## Yang dikerjakan pada sesi 2026-07-26

Empat commit, urut dari yang paling lama:

| Commit | Isi |
|---|---|
| `bb05377` | Verifikasi host key, CSP, keepalive, dan 5 perbaikan bug |
| `8cf9375` | Cari di scrollback, impor `~/.ssh/config`, renderer WebGL (dicabut kemudian) |
| `efcba3b` | Cabut WebGL, lencana rail, nama file potong-tengah, panel bisa diseret |
| `9a024c2` | Kontras teks panel, format tanggal, lebar kolom bisa diatur |

### Keamanan

Sebelum sesi ini Tambat menyambung **tanpa memeriksa identitas server sama
sekali** — tidak ada `known_hosts`, tidak ada fingerprint, tidak ada konfirmasi.
Koneksi bisa disadap tanpa gejala dan password terkirim ke penyadap.

- `src-tauri/src/hostkeys.rs` — fingerprint SHA256 (format `ssh-keygen -lf`) per
  `host:port`, disimpan di `known_hosts.json` milik Tambat sendiri, bukan
  `~/.ssh/known_hosts` milik OpenSSH.
- `src-tauri/src/ssh.rs` — helper `open_session()`: TCP → handshake →
  verifikasi host key → keepalive → auth. Verifikasi **sengaja sebelum auth**,
  supaya kredensial tak pernah terkirim ke server yang identitasnya belum
  terbukti. Dipakai bersama oleh `ssh_connect` dan `panel_open`, jadi tak ada
  jalur yang terlewat.
- `ConnectError` bertipe: varian `HostKey` terpisah dari galat biasa agar
  frontend menampilkan dialog fingerprint, bukan pesan galat mentah.
- `src/components/HostKeyPrompt.tsx` — host baru: minta cocokkan fingerprint.
  Fingerprint berubah: peringatan keras, fingerprint lama ditampilkan tercoret,
  tombol merah. Menyetujui akan menyambung ulang otomatis.
- CSP diisi (sebelumnya `null`), dengan `devCsp` terpisah agar HMR `tauri dev`
  tetap jalan.
- `set_keepalive(true, 30)` — sesi idle tak lagi diputus diam-diam oleh NAT.

### Bug yang diperbaiki

- `Ctrl+Shift+W` tak pernah berfungsi: `e.key === "w"` mustahil cocok saat Shift
  ditekan (`e.key` jadi `"W"`). Dicocokkan lewat `e.code`.
- Terminal tak bisa disalin: tak ada handler clipboard, dan `Ctrl+C` sudah
  berarti SIGINT. Ditambah `Ctrl+Shift+C` / `Ctrl+Shift+V`, dengan fallback
  `execCommand` karena `navigator.clipboard` tak selalu tersedia di WebKitGTK.
- `hosts.json` ditulis non-atomik: crash saat menyimpan bisa menghapus seluruh
  daftar host. Sekarang tulis ke `.tmp` lalu `rename`.
- Keystroke sebelum handshake selesai hilang: `term.onData` didaftarkan sebelum
  `await sshConnect`, input ditahan lalu dikirim setelah sesi siap.
- IPv6 literal `[::1]` ditolak "Alamat tidak valid": kurung siku dilepas di
  `normalize_host`.

### Fitur baru

- **Cari di scrollback** — `Ctrl+Shift+F`, Enter berikutnya, Shift+Enter
  sebelumnya, Esc menutup, dengan penghitung hasil. Dipakai `Ctrl+Shift+F`
  bukan `Ctrl+F` karena `Ctrl+F` punya arti di shell (bash: forward-char).
- **Impor `~/.ssh/config`** — `src-tauri/src/sshconfig.rs`. Membaca
  HostName/User/Port/IdentityFile. Blok berpola wildcard (`Host *`) dilewati.
  Satu baris `Host a b` menghasilkan dua entri. Tanpa direktif `User` dipakai
  nama pengguna lokal, sama seperti OpenSSH. `Include` dan `Match` tidak
  diikuti. Dialog menandai entri yang tujuannya sudah ada di sidebar.
- **Panel file bisa diseret** lebarnya (240–720px), dan lebar kolom Ukuran serta
  Terakhir diubah juga bisa diatur. Semua diingat antar sesi.

### Perbaikan tampilan

- Nama file panjang dipotong di **tengah** lewat dua span CSS, sehingga ekstensi
  tak pernah jadi bagian pertama yang hilang. Ekstensi ganda ikut terbawa
  (`cadangan.sql.gz` → `.sql.gz`); berkas titik seperti `.bashrc` tidak dipecah.
- Kontras dibalik: file biasa tadinya `--muted` sehingga justru paling sulit
  dibaca. Sekarang folder dan file sama-sama putih penuh, pembedanya ikon.
  Berkas tersembunyi tetap diredupkan.
- Tanggal jadi `26-07-26 22:31` (dd-mm-yy jam:menit) berlebar tetap. Tooltip
  tetap tanggal lengkap bernama bulan.
- Lencana sidebar yang menciut jadi dua huruf berwarna (`web-prod` → **WP**),
  warna diturunkan dari `user@host:port` supaya lencana berhuruf sama tetap bisa
  dibedakan. Sebelumnya satu huruf tanpa warna.
- Tombol impor diberi kotak dan label ramah pemula ("Impor host dari komputer
  ini"), bukan tautan telanjang bertulis `~/.ssh/config`.

---

## Keputusan yang sudah diambil — jangan diulang

### Dua koneksi SSH per tab dipertahankan

Satu tab membuka **dua** koneksi SSH ke server yang sama: `ssh_connect`
(terminal/PTY) dan `panel_open` (SFTP + statistik). Akibatnya auth terjadi 2×
per tab — 2 baris login di `w`, 2 hitungan fail2ban, dan 2× prompt kalau server
minta OTP.

**Ini disengaja dan tidak boleh digabung.** Paralelisme terminal ↔ panel adalah
kebutuhan nyata: terminal boleh sibuk (`npm run build`, `tail -f`) sementara
panel tetap listing folder, transfer file, dan poll statistik tiap 5 detik.

Alasan teknisnya: `ssh2` 0.9 menaruh seluruh `Session` di balik satu
`Arc<Mutex<..>>`, jadi semua channel dalam satu koneksi harus antre. Lebih
parah, `Sftp::readdir` (`sftp.rs:244-248`) busy-spin di dalam saat mode
non-blocking — tidak pernah mengembalikan `EAGAIN`, hanya berputar sampai data
datang. Menggabungkan koneksi berarti terminal membeku dan satu core terbakar
setiap kali membaca folder atau memindahkan file. `set_timeout` juga tidak
berlaku di mode non-blocking, jadi semua deadline harus dilacak manual.

Kalau muncul keluhan "diminta OTP dua kali", obatnya adalah dukungan auth
`keyboard-interactive` (lihat daftar sisa di bawah), **bukan** penggabungan
koneksi. Penggabungan yang benar hanya realistis kalau backend pindah ke
`russh` (async, multiplexing channel sungguhan) — itu penulisan ulang besar,
bukan perbaikan bug.

### Renderer WebGL dicabut

`@xterm/addon-webgl` pernah dipasang lalu dicabut lagi di commit yang sama
harinya. Gejalanya: terminal blank hitam setelah pengguna menyetujui dialog
fingerprint. WebKitGTK bisa membuat konteks WebGL yang "berhasil" tapi tidak
menggambar apa pun, dan `try/catch` di sekitar konstruktor tidak menangkap
kegagalan diam seperti itu. Setelah addon dilepas, gejalanya hilang —
dikonfirmasi oleh pengguna.

Kalau ingin mencoba akselerasi lagi, siapkan cara mendeteksi kanvas yang tidak
menggambar (bukan hanya konstruktor yang gagal) dan sediakan setelan untuk
mematikannya.

---

## Sisa temuan audit yang belum dikerjakan

Urut dari yang paling berdampak:

1. **Auth `keyboard-interactive` tidak didukung** (`src-tauri/src/ssh.rs`) —
   hanya `password`/`pubkey`/`agent`. Server yang mematikan
   `PasswordAuthentication` tapi menyalakan `KbdInteractiveAuthentication`
   (umum di VPS, dan wajib untuk 2FA) **tidak bisa dimasuki sama sekali**.
2. **Panel bergantung pada shell remote** — `cp`/`mv`/`rm` dan statistik
   dijalankan lewat `run_sh` + `exec "sh"`. Server SFTP-only
   (`ForceCommand internal-sftp`) membuat semua operasi itu gagal. SFTP punya
   primitif untuk rename/remove yang belum dipakai.
3. **Renderer terminal** masih DOM sepenuhnya (lihat keputusan WebGL di atas).

### Fitur ala Termius yang belum ada

Port forwarding (local/remote/dynamic) · jump host / ProxyJump · grup atau tag
host · split pane · snippet perintah · halaman setelan (font, tema, scrollback) ·
reconnect otomatis · indikator progres transfer file.

---

## Cacat kecil yang diketahui

- Pada nama file yang terpotong, ada celah selebar ±1 karakter antara elipsis
  dan ekstensi (`lapo… .xlsx`). Elipsis CSS berhenti di batas kotak yang tidak
  pas kelipatan lebar karakter. Menghilangkannya butuh potong-nama yang dihitung
  di JS memakai metrik font sebenarnya.
- Buffer keystroke saat masih connecting belum diverifikasi secara manual —
  mock sshd tidak meng-echo input sehingga tidak terlihat di layar. Logikanya
  ada di `src/components/TermView.tsx`.
- Rail sidebar yang menciut memakai daftar `hosts`, bukan hasil pencarian
  `filtered` (`src/components/Sidebar.tsx`). Tidak terlihat karena kotak cari
  ikut tersembunyi saat menciut. Ini bukan bug baru.
- `pingGoogleMs` dihitung backend tapi tidak dipakai UI — hanya `pingCfMs` yang
  ditampilkan.
