# Panduan Pengguna Tidyr

Tidyr adalah aplikasi desktop yang aman, ramah pengguna, dan 100% lokal untuk merapikan folder yang berantakan (seperti Unduhan/Downloads, Desktop, WhatsApp Media, atau Dokumen) serta mengatasi berkas kembar (duplikat) tanpa risiko kehilangan data penting.

Seluruh teks antarmuka Tidyr disajikan dalam **Bahasa Indonesia**.

---

## 1. Alur Penggunaan Langkah demi Langkah

### Langkah 1: Beranda (Pilih Folder)
1. Buka aplikasi **Tidyr**.
2. Klik tombol **"Pilih folder"** untuk memilih folder yang ingin dirapikan lewat dialog bawaan sistem, atau **seret dan lepas (drag-and-drop)** folder langsung ke jendela aplikasi.
3. Pilih **Mode merapikan**:
   * **Berdasarkan jenis file** (`ByType`): Mengelompokkan berkas ke folder kategori (`Dokumen`, `Gambar`, `Video`, `Audio`, `Arsip`, `Installer`). Tangkapan layar otomatis ditempatkan ke `Gambar/Screenshot`.
   * **Berdasarkan tanggal** (`ByDate`): Khusus foto dan video, berkas diurutkan rapi ke dalam folder berdasarkan bulan pengambilan foto (`Gambar/YYYY/MM - NamaBulan`), sedangkan berkas lainnya dirapikan menurut jenisnya.
4. Klik tombol **"Analisis"**.

> **Keamanan**: Tidyr menolak pemindaian pada folder sistem kritis seperti `C:\Windows`, `Program Files`, `/`, `/etc`, maupun folder home utama demi menjaga kestabilan perangkat Anda.

---

### Langkah 2: Memindai (Dry-Run Tanpa Mengubah Berkas)
Proses pemindaian berjalan di latar belakang:
* Memeriksa seluruh isi folder secara mendalam.
* Menghitung hash berkas untuk mendeteksi duplikat persis secara bertahap.
* Mengidentifikasi installer usang yang berusia lebih dari 60 hari.
* **Penting**: Pada tahap ini, **tidak ada berkas yang diubah, dipindah, atau dihapus**. Anda dapat membatalkan pemindaian kapan saja dengan menekan tombol **"Batal"**.

---

### Langkah 3: Rencana (Periksa Hasil Sebelum Bertindak)
Setelah pemindaian selesai, layar **Rencana** akan menampilkan pratinjau lengkap perubahan:
* **Kartu Ringkasan**: Menampilkan total berkas yang ditemukan, jumlah duplikat, potensi ruang penyimpanan yang dihemat, serta aksi aktif.
* **Grup Duplikat**: Untuk setiap kelompok berkas kembar, Tidyr memberikan lencana **"Dipertahankan"** pada berkas yang paling aman (misalnya berkas yang berada di folder lebih rapi atau memiliki nama asli). Anda bebas memindahkan status yang dipertahankan ke berkas lain dengan menekan tombol **"Jadikan yang dipertahankan"**. Terdapat pula kotak centang grup untuk membatalkan aksi pada kelompok tersebut.
* **Tab Kategori**: Anda dapat memfilter aksi berdasarkan tab `Semua`, `Duplikat`, `Rapikan`, `Installer lama`, dan `Folder kosong`.
* **Daftar Aksi Virtual**: Aksi pindah menampilkan arah yang jelas (`dari ... → ke ...`). Anda dapat memilih atau membatalkan pilihan aksi satu per satu maupun secara keseluruhan lewat tombol **"Pilih semua"** atau **"Batal pilih semua"**.
* **Konfirmasi Operasi Besar**: Jika rencana melibatkan lebih dari 50 karantina atau lebih dari 1 GB, Tidyr akan memunculkan dialog konfirmasi tambahan demi memastikan persetujuan Anda.

---

### Langkah 4: Menerapkan Rencana
1. Klik tombol **"Terapkan rencana"**.
2. Tidyr menjalankan aksi terpilih dengan pencatatan jurnal aman (*write-ahead journal*):
   * Setiap berkas diverifikasi ulang ukuran dan waktu modifikasinya tepat sebelum dipindah. Jika berkas baru saja diubah oleh aplikasi lain atau sedang dikunci/dipakai, aksi tersebut dilewati secara otomatis (`"dilewati: sedang dipakai"`).
   * Nama berkas tujuan tidak akan pernah menimpa berkas yang sudah ada (menggunakan penambahan nomor otomatis seperti `nama (2).ext`).
   * Pembatalan di tengah jalan akan berhenti secara konsisten dan aman.

---

### Langkah 5: Selesai & Riwayat
1. Setelah penerapan tuntas, layar **Selesai** merangkum jumlah berkas yang dipindah dan dikarantina.
2. Terdapat tombol **"Urungkan sesi ini"** untuk mengembalikan seluruh perubahan seketika ke posisi semula.

---

## 2. Cara Kerja Fitur "Urungkan" (Rollback 1-Klik)

Semua sesi yang dijalankan oleh Tidyr dicatat ke dalam database jurnal transaksi lokal (`journal.db`).
* **Satu Kali Klik**: Di tab **Riwayat**, setiap sesi yang pernah dijalankan memiliki tombol **"Urungkan"**.
* **Pemulihan Sempurna**: Saat diurungkan, seluruh berkas yang dipindah dikembalikan ke lokasi awalnya, dan berkas yang dikarantina dikembalikan ke folder asalnya dengan hash dan timestamp asli.
* **Pemulihan Setelah Crash**: Jika komputer mati mendadak atau aplikasi terhenti paksa di tengah proses penerapan rencana, saat Tidyr dibuka kembali aplikasi akan otomatis memunculkan dialog:
  **"Sesi sebelumnya terhenti"** dengan tombol **"Kembalikan semua"**. Seluruh berkas yang sempat dipindah akan otomatis dikembalikan ke kondisi awal tanpa meninggalkan status setengah jalan.

---

## 3. Lokasi & Manajemen Karantina

Tidyr **tidak pernah menghapus berkas secara permanen** pada saat pembersihan folder. Berkas yang disingkirkan (seperti salinan duplikat) dipindahkan ke folder Karantina aplikasi:

* **Lokasi Karantina**:
  * **Windows**: `%LOCALAPPDATA%\Tidyr\quarantine\`
  * **Linux/macOS**: `~/.local/share/tidyr/quarantine/`
* **Masa Retensi**: Bawaan 30 hari (dapat diubah antara 1 hingga 365 hari di Pengaturan).
* **Pemberitahuan Startup**: Ketika masa retensi terlewati, berkas yang kedaluwarsa dibersihkan dan Tidyr menampilkan notifikasi transparan di bagian atas jendela (misal: *"3 file karantina kedaluwarsa dihapus permanen (15 MB)"*). Pembersihan tidak pernah dilakukan diam-diam.
* **Pemulihan Berkas Tunggal**: Anda dapat menelusuri daftar berkas karantina satu per satu di tab **Riwayat** dan memulihkan berkas tertentu kapan pun dibutuhkan.
* **Buka Folder Langsung**: Pada tab **Pengaturan**, terdapat tombol **"Buka folder"** untuk membuka folder karantina langsung di File Explorer (Windows) atau Finder (macOS).

---

## 4. Pengaturan Tambahan

Pada tab **Pengaturan**, Anda dapat mengonfigurasi:
1. **Folder yang Dikecualikan**: Tambahkan path folder yang tidak boleh disentuh atau dipindai oleh Tidyr. Folder ini akan dilewati secara otomatis saat proses pemindaian.
2. **Masa Retensi Karantina**: Menentukan berapa hari berkas duplikat disimpan di karantina sebelum dihapus permanen.
3. **Pembaruan Otomatis**: Memeriksa pembaruan rilis resmi di GitHub maksimal sekali setiap 24 jam. Jika versi baru tersedia, banner pembaruan akan memberi Anda pilihan:
   * **Perbarui sekarang**: Mengunduh dan memperbarui aplikasi secara otomatis.
   * **Nanti**: Menunda pembaruan.
   * **Lewati versi ini**: Mengabaikan versi tersebut hingga rilis berikutnya.
4. **Tema**: Mendukung mode Sistem, Terang (*Light*), dan Gelap (*Dark*).

---

## 5. Unduh & Instalasi Aplikasi

Unduh berkas instalasi resmi dari halaman rilis GitHub:
👉 **[Unduh Rilis Resmi Tidyr](https://github.com/baniputrabangsawan/Tidyr/releases/latest)**

### Tabel Format Paket & Panduan Pasang/Copot

| Sistem Operasi | Format Berkas | Cara Pasang (*Install*) | Cara Copot (*Uninstall*) | Dukungan Pembaruan Otomatis |
|---|---|---|---|---|
| **Windows** | `.exe` (NSIS) | Jalankan `tidyr-setup.exe` (terpasang di `%LOCALAPPDATA%\Programs\Tidyr`, tanpa perlu hak Administrator) | Pengaturan Windows → Aplikasi → Aplikasi terinstal → Cari "Tidyr" → Copot pemasangan (*Uninstall*) | **Ya** (Pembaruan otomatis bawaan dengan verifikasi tanda tangan digital) |
| **Windows** | `.msi` | Jalankan `tidyr_x64.msi` | Pengaturan Windows → Aplikasi → Aplikasi terinstal → Cari "Tidyr" → Uninstall | Manual / Dikelola kebijakan grup (GPO) |
| **Linux** | `.deb` (Debian, Ubuntu, Mint) | `sudo dpkg -i tidyr_*_amd64.deb` atau `sudo apt install ./tidyr_*_amd64.deb` | `sudo apt remove tidyr` | **Tidak** (Dikelola oleh manajer paket sistem; perbarui dengan mengunduh `.deb` versi baru) |
| **Linux** | `.AppImage` | Berikan izin eksekusi (`chmod +x tidyr_*.AppImage`) lalu jalankan langsung | Hapus berkas `.AppImage` | Pembaruan langsung saat aplikasi dijalankan |

### Penanganan Peringatan Windows SmartScreen
Karena aplikasi komunitas sumber terbuka ini belum ditandatangani menggunakan sertifikat komersial berbayar (EV Code Signing), Windows SmartScreen mungkin memunculkan layar biru bertuliskan: *"Windows protected your PC"* atau *"Windows melindungi PC Anda"*.
1. Klik **"More info"** (*Info selengkapnya*).
2. Klik tombol **"Run anyway"** (*Tetap jalankan*).

### Lokasi Penyimpanan Data Pengguna
Tidyr menyimpan jurnal transaksi, riwayat sesi, cache hash, dan karantina secara lokal di komputer Anda:
* **Windows**: `%LOCALAPPDATA%\id.tidyr.app` (`C:\Users\<NamaUser>\AppData\Local\id.tidyr.app`)
* **Linux**: `~/.local/share/id.tidyr.app`
* **macOS**: `~/Library/Application Support/id.tidyr.app`

Isi folder data:
* `journal.db` — Jurnal transaksi SQLite WAL untuk fitur **Urungkan 1-Klik**.
* `quarantine/` & `quarantine.db` — Folder isolasi berkas duplikat yang aman.
* `cache.db` — Cache hash BLAKE3 untuk mempercepat pemindaian ulang.
* `settings.json` — Konfigurasi preferensi dan daftar folder pengecualian.

### Catatan Transparan Pembaruan Aplikasi
* **Windows**: Pembaruan otomatis memeriksa versi baru secara berkala (sekali per 24 jam) saat aplikasi dibuka, memvalidasi tanda tangan kriptografi rilis, dan mengunduh pembaruan di tempat.
* **Linux (`.deb`)**: Aplikasi tidak melakukan auto-update di latar belakang karena paket Debian wajib dikelola oleh manajer paket sistem (`apt`/`dpkg`). Untuk memperbarui versi, unduh berkas `.deb` rilis terbaru dari GitHub Releases dan pasang kembali.

---

## 6. Lisensi

Aplikasi Tidyr didistribusikan di bawah lisensi resmi **MIT License**.
Hak Cipta: **Copyright (c) 2026 Bani Putra Bangsawan**.

Rincian lisensi dependensi sumber terbuka pihak ketiga dapat dilihat pada berkas [`THIRD-PARTY-LICENSES.md`](../../THIRD-PARTY-LICENSES.md).
