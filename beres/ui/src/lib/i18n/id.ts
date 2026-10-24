// Indonesian UI strings. All user-facing text lives here so English can be
// added later by mirroring this module (design the i18n layer for that).

export const id = {
  appName: "Beres",
  tagline: "Rapikan folder tanpa takut kehilangan file",

  nav: {
    home: "Beranda",
    plan: "Rencana",
    applying: "Menerapkan",
    done: "Selesai",
    history: "Riwayat",
    settings: "Pengaturan",
  },

  onboarding: [
    {
      title: "Rencana dulu, aksi belakangan",
      body: "Pilih folder, klik Analisis, dan periksa rencananya. Tidak ada yang berubah sebelum Anda setuju.",
    },
    {
      title: "Tidak ada yang terhapus permanen",
      body: "File yang disingkirkan masuk karantina 30 hari dan bisa dikembalikan kapan saja.",
    },
    {
      title: "Semua bisa diurungkan",
      body: "Setiap sesi punya tombol Urungkan satu klik di layar Riwayat.",
    },
  ],
  onboardingNext: "Lanjut",
  onboardingStart: "Mulai",

  home: {
    chooseFolder: "Pilih folder",
    dropHint: "atau seret folder ke jendela ini",
    recent: "Terakhir dibuka",
    analyze: "Analisis",
    mode: "Mode merapikan",
    modeByType: "Berdasarkan jenis file",
    modeByDate: "Berdasarkan tanggal (foto & video)",
    empty: "Belum ada folder. Pilih folder untuk mulai.",
    protectedError:
      "Folder ini dilindungi sistem dan tidak bisa dirapikan. Pilih folder seperti Unduhan atau Dokumen.",
  },

  scan: {
    title: "Memindai…",
    files: "file dipindai",
    phase_scan: "Memindai file",
    phase_hash: "Mencari file kembar",
    phase_plan: "Menyusun rencana",
    cancel: "Batal",
    unreadable: "tidak bisa dibaca",
    symlinks: "tautan dilewati",
    emptyFiles: "file kosong",
  },

  plan: {
    title: "Rencana",
    summaryFiles: "file ditemukan",
    tabs: {
      duplikat: "Duplikat",
      rapikan: "Rapikan",
      installer: "Installer lama",
      folder_kosong: "Folder kosong",
    },
    selectAll: "Pilih semua",
    deselectAll: "Batal pilih semua",
    keeper: "Dipertahankan",
    setKeeper: "Jadikan yang dipertahankan",
    before: "Sekarang",
    after: "Sesudah",
    apply: "Terapkan rencana",
    confirmTitle: "Yakin terapkan?",
    confirmBody:
      "Rencana ini besar. Periksa lagi — semua aksi bisa diurungkan, tapi lebih aman memastikan dulu.",
    confirmYes: "Ya, terapkan",
    confirmNo: "Kembali",
    empty: "Tidak ada aksi pada tab ini. Semua sudah rapi!",
  },

  applying: {
    title: "Menerapkan rencana…",
    cancel: "Batal",
    cancelled: "Dibatalkan. Semua yang sudah berjalan tetap bisa diurungkan.",
  },

  done: {
    title: "Selesai!",
    moved: "file dipindah",
    quarantined: "file dikarantina",
    undo: "Urungkan sesi ini",
    backHome: "Kembali ke Beranda",
    skipped: "dilewati",
  },

  history: {
    title: "Riwayat",
    undo: "Urungkan",
    undone: "Sudah diurungkan",
    empty: "Belum ada sesi. Terapkan sebuah rencana dulu.",
    quarantinedFiles: "File karantina",
    restore: "Pulihkan",
    restoredTo: "Dipulihkan ke",
    crashTitle: "Ada sesi yang terputus",
    crashBody:
      "Aplikasi sepertinya berhenti saat menerapkan rencana. Pulihkan atau selesaikan?",
    crashRollback: "Kembalikan seperti semula",
  },

  settings: {
    title: "Pengaturan",
    quarantineDir: "Lokasi karantina",
    retention: "Karantina disimpan (hari)",
    excluded: "Folder yang dikecualikan",
    autoUpdate: "Periksa pembaruan otomatis",
    language: "Bahasa",
    theme: "Tema",
    themeSystem: "Sistem",
    themeLight: "Terang",
    themeDark: "Gelap",
    openDataDir: "Buka folder data aplikasi",
    checkUpdate: "Periksa pembaruan",
    updateAvailable: "Versi baru tersedia",
    updateNow: "Perbarui sekarang",
    updateLater: "Nanti",
    updateSkip: "Lewati versi ini",
    save: "Simpan",
    saved: "Pengaturan disimpan",
    privacyNote:
      "Beres 100% lokal. Satu-satunya koneksi jaringan adalah pemeriksaan pembaruan.",
  },

  common: {
    loading: "Memuat…",
    error: "Terjadi kesalahan",
    copyDetails: "Salin detail",
    copied: "Detail disalin",
    retry: "Coba lagi",
    close: "Tutup",
    bytes: (n: string) => n,
  },
} as const;

export type Strings = typeof id;
