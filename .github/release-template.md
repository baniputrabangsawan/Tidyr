# Catatan Rilis Tidyr v{VERSION}

> ⚠️ **Penting**: Selalu **cadangkan (backup) folder penting** sebelum pertama kali mencoba merapikan folder berukuran besar. Meskipun Tidyr menerapkan garansi keamanan ketat (*write-ahead journal* dan *quarantine-first*), pencadangan berkas pribadi adalah praktik terbaik.

---

## 🚀 Yang Baru di Versi Ini
- **Fitur Baru & Peningkatan**:
  - Ringkasan fitur utama yang baru ditambahkan...
  - Peningkatan performa pemindaian dan antarmuka pengguna...
- **Perbaikan Bug (*Bug Fixes*)**:
  - Daftar perbaikan cacat atau error yang telah diselesaikan...

---

## 📥 Panduan Unduh & Instalasi

Unduh paket installer resmi dari bagian **Assets** di bawah atau melalui tautan berikut:
👉 **[Halaman Rilis Resmi Tidyr](https://github.com/baniputrabangsawan/Tidyr/releases/latest)**

| Sistem Operasi | Berkas Unduhan | Cara Pasang (*Install*) |
|---|---|---|
| **Windows** | `tidyr-setup.exe` (NSIS) | Jalankan installer; terpasang ke folder lokal pengguna (`%LOCALAPPDATA%\Programs\Tidyr`) tanpa memerlukan hak Administrator. |
| **Windows MSI** | `tidyr_x64.msi` | Jalankan paket MSI untuk instalasi standar Windows. |
| **Linux (.deb)** | `tidyr_{VERSION}_amd64.deb` | Pasang via terminal: `sudo dpkg -i tidyr_{VERSION}_amd64.deb` atau `sudo apt install ./tidyr_{VERSION}_amd64.deb` |
| **Linux AppImage** | `tidyr_{VERSION}_amd64.AppImage` | Berikan izin eksekusi (`chmod +x tidyr_*.AppImage`) dan jalankan langsung. |

### Penanganan Peringatan Windows SmartScreen
Jika muncul layar biru Windows SmartScreen (*"Windows protected your PC"* atau *"Windows melindungi PC Anda"*):
1. Klik teks **"More info"** (*Info selengkapnya*).
2. Klik tombol **"Run anyway"** (*Tetap jalankan*).

---

## 📂 Lokasi Penyimpanan Data Pengguna
Tidyr menyimpan riwayat transaksi lokal di:
* **Windows**: `%LOCALAPPDATA%\id.tidyr.app` (`C:\Users\<User>\AppData\Local\id.tidyr.app`)
* **Linux**: `~/.local/share/id.tidyr.app`
* **macOS**: `~/Library/Application Support/id.tidyr.app`

Isi data:
* `journal.db` — Jurnal transaksi aman untuk fitur **Urungkan 1-Klik**.
* `quarantine/` & `quarantine.db` — Berkas kembar yang dikarantina secara terisolasi.
* `cache.db` — Cache hash BLAKE3 untuk akselerasi pemindaian ulang.
* `settings.json` — Konfigurasi preferensi pengguna.

---

## ⚠️ Batasan yang Diketahui (*Known Limitations*)
- Deteksi duplikat difokuskan pada berkas identik persis 100% (*exact duplicates* berbasis hash BLAKE3 bertahap). Pencocokan foto mirip (*perceptual hash*) dan OCR tidak termasuk dalam rilis ini.
- Tidyr menolak pemindaian pada folder sistem terlindungi (`C:\Windows`, `Program Files`, `/`, `/etc`, drive root).
- Pembaruan otomatis bawaan aktif pada paket Windows NSIS; untuk paket Linux `.deb`, pembaruan dilakukan secara manual dengan memasang berkas `.deb` rilis terbaru.
