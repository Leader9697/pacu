# Pacu — booster gaming yang ringan untuk Windows

Pacu adalah aplikasi seperti Process Lasso yang difokuskan untuk gaming, sekarang digabung dengan **Pacu Volt** (undervolt GPU NVIDIA dan AMD otomatis) dan **monitor + overlay** seperti RivaTuner (FPS, suhu, clock CPU/GPU di dalam game). Tersedia sebagai installer `Pacu-Setup.exe` atau satu file portable `Pacu.exe` (±7 MB), tanpa runtime tambahan dan tanpa service terpisah.

## Instalasi
Unduh dari [GitHub Releases](https://github.com/Leader9697/pacu/releases/latest):
- **`Pacu-Setup.exe` (disarankan)**: memasang Pacu ke `C:\Program Files\Pacu`, membuat shortcut di Start Menu dan desktop (bisa dimatikan), dan mendaftarkan Pacu di **Settings → Apps** supaya bisa di-uninstall seperti aplikasi lain. Installer menutup Pacu yang sedang berjalan, jadi bisa dipakai juga untuk memasang versi baru di atas yang lama. `Pacu-Setup.exe /S` memasang tanpa jendela.
- **`Pacu.exe` (portable)**: langsung jalan dari folder mana pun tanpa dipasang.

Uninstall menutup Pacu, menghapus tugas Task Scheduler milik Pacu (jalan saat login dan undervolt saat login), lalu bertanya apakah pengaturan di `%APPDATA%\Pacu` dan `%APPDATA%\PacuVolt` ikut dihapus (bawaannya disimpan). Tweak Windows yang sudah diterapkan tidak dikembalikan otomatis; kembalikan dulu dari dalam Pacu bila perlu.

Pindah dari portable ke installer: tutup Pacu portable, jalankan installer, lalu buka Pacu dari Start Menu. Pengaturan terbawa karena disimpan di `%APPDATA%`, dan tugas jalan-saat-login otomatis diarahkan ke salinan yang terpasang.

## Cara pakai

1. Buka Pacu dari Start Menu (atau jalankan `Pacu.exe` versi portable). Windows akan meminta izin admin, karena mengatur prioritas proses lain butuh hak admin.
2. Main game seperti biasa. Mode Game menyala sendiri saat game terdeteksi lalu mati saat game ditutup.
3. Supaya ikut menyala saat Windows hidup: **Pengaturan → Jalankan saat Windows menyala**. Pacu memakai Task Scheduler sehingga tidak ada prompt UAC tiap kali login.
4. Tombol tutup (X) menyembunyikan Pacu ke system tray. Klik dua kali ikon tray untuk membukanya lagi. Pilih **Keluar** dari menu tray untuk benar-benar menutup.

Semua perubahan Mode Game bersifat sementara: saat game ditutup atau Pacu keluar, prioritas, power plan, timer, dan aplikasi yang dibekukan dikembalikan seperti semula.

### Mulai cepat untuk Deadlock
1. **Mode Game → Preset sekali klik → Deadlock.** Ini menambahkan `project8.exe` dan menyalakan semua optimasi yang cocok.
2. **Optimasi Windows → Terapkan semua**, lalu restart PC.
3. Main Deadlock. Buka **FPS & Benchmark** untuk melihat FPS dan 1% low, dan rekam benchmark sebelum/sesudah mengubah pengaturan.

## Tampilan
Mengikuti sistem desain Fluent Windows 11 (seperti aplikasi Settings dan Task Manager): font Segoe UI dari Windows, ikon Fluent System Icons, warna aksen mengikuti Personalisasi Windows, menu samping berkelompok, kartu pengaturan dengan sakelar Aktif/Nonaktif. Halaman Monitor memakai tabel sensor ala HWiNFO (Sekarang, Minimum, Maksimum, Rata-rata sejak Pacu dibuka, bisa direset).

## Pembaruan
Pacu memeriksa rilis baru di [GitHub Releases](https://github.com/Leader9697/pacu/releases) saat menyala dan tiap 6 jam (bisa dimatikan di Pengaturan). Unduhan lewat HTTPS dicocokkan dengan SHA-256 dari rilis, lalu exe lama diganti setelah kamu menekan "Mulai ulang". Ini berlaku untuk versi portable maupun yang dipasang lewat installer; versi di Settings → Apps ikut diperbarui. Rilis dibuat otomatis oleh GitHub Actions setiap version di `Cargo.toml` dinaikkan dan di-push ke `main`.


### Mode Game
- **Deteksi otomatis** lewat tiga cara: daftar game kamu sendiri, game di folder Steam / Epic / Riot / Xbox / GOG / Ubisoft / EA / Battle.net, atau aplikasi layar penuh di depan (browser dan pemutar video dikecualikan).
- **Game diberi prioritas penuh**: CPU Tinggi, I/O (disk) Tinggi, GPU Tinggi, dan Efficiency Mode Windows dimatikan untuk game.
- **Power plan "Pacu Performa"** dibuat otomatis (salinan Ultimate/High Performance) dengan core parking mati, CPU selalu 100%, boost agresif, PCIe dan USB tanpa hemat daya, dan disk tidak pernah tidur. Dipakai selama bermain lalu dikembalikan. Power plan hanya dikembalikan kalau kamu tidak menggantinya sendiri di tengah jalan.
- **Resolusi timer 0,5 ms** supaya frame time lebih stabil.
- **Cegah sleep / layar mati** selama bermain.
- **Bersihkan RAM standby** saat mulai bermain dan berkala tiap 30 detik selama bermain, hanya bila RAM bebas tinggal sedikit.
- **Bekukan aplikasi latar** (OneDrive, Widgets, Phone Link, Google Drive, Dropbox, updater Edge/Google/Adobe, dan daftar buatanmu sendiri): dijeda total selama bermain dan dilanjutkan saat game ditutup. Bila kamu membuka salah satunya ke depan, ia dilanjutkan otomatis. Bila Pacu tertutup paksa, aplikasi itu dilanjutkan saat Pacu dibuka lagi.
- **Preset sekali klik** untuk Deadlock, Counter-Strike 2, Dota 2, Valorant, Apex Legends, dan Fortnite.
- **Redam aplikasi latar lebih cepat** selama bermain: ambang penyeimbang dipotong setengah.
- **Khusus CPU Intel hybrid (P-core + E-core)**: aplikasi latar yang diredam dipindah ke E-core supaya P-core bebas untuk game. Ada juga opsi untuk menjalankan game hanya di P-core.
- Tombol **Otomatis / Nyala / Mati** di Beranda dan di menu tray.

### Monitor & Overlay (baru di v0.3)
- Satu halaman untuk semua sensor: FPS, 1% low, frame time, GPU (pemakaian, suhu, clock, daya, tegangan, kipas), VRAM, CPU (pemakaian, suhu, clock, daya paket), dan RAM, plus grafik 2 menit terakhir.
- **Overlay di dalam game**, dinyalakan dari halaman ini. Ada dua cara menampilkannya:
  - **Lewat RTSS** (bila RivaTuner Statistics Server sedang berjalan): teks Pacu dititipkan ke salah satu slot OSD RTSS, jadi tampil di dalam game di semua mode layar termasuk Fullscreen eksklusif. FPS dan frame time dihitung RTSS untuk game itu sendiri.
  - **Jendela Pacu**: kotak kecil transparan yang selalu di atas dan tidak bisa diklik, di pojok layar yang dipilih. Tidak butuh RTSS dan tidak menyentuh game, tetapi hanya terlihat di mode Windowed / Borderless.
  - Mode **Otomatis** (bawaan) memakai RTSS bila sedang berjalan, selain itu jendela Pacu.
- **Ctrl+Shift+F10** menyembunyikan / menampilkan overlay.
- FPS diambil dari PresentMon untuk game yang dideteksi Mode Game, atau dari RTSS untuk aplikasi 3D lain yang sedang di depan.
- Monitor berjalan di thread sendiri, jadi overlay tetap diperbarui walaupun Pacu tersembunyi di tray.
- **Suhu dan daya CPU butuh driver PawnIO**, driver gratis bertanda tangan yang juga dipakai LibreHardwareMonitor dan FanControl. Modul sensornya (`IntelMSR.bin` untuk Intel, `AMDFamily17.bin` untuk Ryzen) sudah ada di dalam Pacu; driver-nya dipasang sekali lewat tombol **Pasang PawnIO (winget)** di halaman Monitor. Setelah terpasang, suhu CPU muncul sendiri dalam satu menit tanpa restart. Clock CPU dan semua sensor GPU tidak butuh PawnIO.

### Undervolt GPU (Pacu Volt)
Semua fitur Pacu Volt sekarang ada di halaman **Undervolt GPU**, dengan tab Beranda, Cari Titik Optimal, Kurva V/F, dan Log & Diagnostik. Cara kerja dan pengamannya dijelaskan di tab Beranda halaman itu.
- Profil, sesi pencarian, dan pengaturan dari `PacuVolt.exe` lama tetap dipakai (folder data tetap `%APPDATA%\PacuVolt`).
- **Pasang otomatis saat Windows menyala** kini menjalankan `Pacu.exe --volt-apply`. Bila sebelumnya sakelar ini dinyalakan di PacuVolt.exe, Pacu mengarahkan tugasnya ke Pacu.exe sendiri saat dibuka, jadi PacuVolt.exe boleh dihapus.
- Menyembunyikan Pacu ke tray tidak menghentikan pencarian yang sedang berjalan. Memilih **Keluar** dari tray menghentikannya dan mengembalikan kurva ke bawaan.
- `Pacu.exe --volt-apply` memasang profil lalu keluar, `Pacu.exe --volt-reset` mengembalikan kurva ke bawaan.

#### Overclock VRAM (baru, GPU NVIDIA)
Tab **Overclock VRAM** mencari offset clock memori secara otomatis.
- Memori GDDR6/GDDR6X mengoreksi error sendiri (EDC): clock yang terlalu tinggi membuat bandwidth turun sebelum muncul artefak. Karena itu Pacu mengukur bandwidth nyata dengan compute shader D3D11 yang membaca dan menulis 2 x 128 MB sambil memeriksa setiap nilai.
- Offset dinaikkan per +100 MHz (satuan sama dengan Afterburner) selama bandwidth tidak turun lebih dari 1,5% dari titik terbaik dan tidak ada error data, TDR, atau event driver. Setelah gagal, Pacu mencoba titik tengah (+50), lalu memakai titik terbaik dikurangi margin 50 MHz dan memvalidasinya 60 detik.
- Hasil disimpan di `%APPDATA%\PacuVolt\vram.json` dan ikut dipasang oleh `Pacu.exe --volt-apply` (sakelar "Pasang otomatis saat Windows menyala").
- Offset memori ditulis lewat NvAPI_GPU_SetPstates20, ID yang sama dengan yang dipakai Afterburner (dicek di RTHAL.dll); tata letak struct dari NVAPI SDK publik.

#### GPU AMD Radeon (baru)
- Didukung: RX 5000 dan RX 6000 (RDNA1/RDNA2), misalnya RX 6700 XT. Dibutuhkan driver AMD Adrenalin terbaru, karena Pacu memakai ADLX (`amdadlx64.dll`, ikut terpasang bersama driver).
- Belum didukung: RX 7000 dan RX 9000. Kartu ini memakai offset tegangan, bukan tegangan maksimum; Pacu menampilkan pesan bila mendeteksinya.
- AMD tidak membuka kurva V/F per titik. Yang diatur adalah dua slider yang sama dengan **Adrenalin → Performa → Tuning → Tuning GPU manual**: **Tegangan maksimum** dan **Frekuensi maksimum**. Frekuensi maksimum dibiarkan di nilai bawaan, lalu tegangan maksimum diturunkan bertahap (beban Unigine Heaven + pemeriksa error D3D11) sampai muncul tanda tidak stabil, lalu dinaikkan lagi dengan margin.
- Halaman Kurva V/F di GPU AMD menampilkan model dari dua slider itu, bukan kurva asli dari driver.
- Nilai bawaan kartu dibaca saat Pacu pertama kali melihat kartu dalam kondisi pabrik dan disimpan di `%APPDATA%\PacuVolt\amd-bawaan.json`. Bila tuning manual sudah diubah di Adrenalin sebelum membuka Pacu, tekan **Reset** di Adrenalin dulu.
- Jangan biarkan Adrenalin memasang profil tuning sendiri bersamaan dengan Pacu.
- Profil dicatat per nama GPU. Profil dari kartu NVIDIA lama tidak akan dipasang ke kartu AMD.

### FPS & Benchmark
- FPS rata-rata, 1% low, dan grafik frame time langsung, diukur dengan PresentMon (Intel) yang sudah dibundel di dalam Pacu. Tidak ada overlay atau injeksi ke game, jadi aman untuk anti-cheat seperti VAC.
- Rekam benchmark 30 detik sampai 5 menit dengan nama, lalu bandingkan hasilnya (FPS rata-rata, 1% low, 0,1% low) dalam persen terhadap rekaman pertama untuk game yang sama. Hasil disimpan di `%APPDATA%\Pacu\benchmark.json`.

### Optimasi Windows
Pengaturan untuk gaming, masing-masing dengan status dan tombol Terapkan / Kembalikan. Nilai lama disimpan di `%APPDATA%\Pacu\tweaks-backup.json`.
- Matikan perekaman latar Xbox Game Bar (Game DVR)
- Nyalakan Game Mode Windows
- Nyalakan Hardware-accelerated GPU scheduling (perlu restart)
- Nyalakan Optimizations for windowed games
- Izinkan resolusi timer global (perlu restart)
- Prioritaskan game di penjadwal multimedia (MMCSS) (perlu restart)
- Matikan power throttling Windows (perlu restart)
- **Profil driver NVIDIA untuk Deadlock** (baru): sama seperti Program Settings di NVIDIA Control Panel untuk `project8.exe`. Isinya Power management Prefer maximum performance, Low Latency Mode On, Threaded optimization On, dan Shader Cache On. Ditulis lewat NvAPI_DRS (NVAPI SDK publik). Bila belum ada profil yang memuat `project8.exe`, Pacu membuat profil "Pacu - project8.exe". Kembalikan menghapus nilai itu sehingga driver memakai bawaannya lagi.
- Matikan akselerasi mouse

**Ekstrem** (baru di v0.5.0, idenya diadaptasi dari playbook [AtlasOS](https://github.com/atlas-os/atlas), ditulis ulang untuk Pacu). Punya tombol "Terapkan semua" sendiri, dan setiap baris bisa dikembalikan:
- Prioritaskan jendela aktif di penjadwal CPU (`Win32PrioritySeparation` 0x26)
- Fullscreen eksklusif asli dan Game Bar mati total (FSO mati, kebijakan `AllowGameDVR` 0)
- Kunci kernel di RAM dan matikan penggabungan halaman (`DisablePagingExecutive`, `DisablePageCombining`) (perlu restart)
- Matikan Fault Tolerant Heap (perlu restart)
- NTFS lebih ringan: tanpa pencatatan waktu akses terakhir dan tanpa nama pendek 8.3 (perlu restart)
- Matikan animasi dan efek visual (berlaku setelah keluar-masuk akun)
- Hapus jeda aplikasi startup
- Matikan PC lebih cepat (aplikasi/layanan macet ditunggu 2 detik)
- Matikan Fast Startup

Power plan "Pacu Performa" juga ikut dari AtlasOS: CPU tidak masuk throttle state, interval cek performa 200 ms, NVMe tidak pernah idle, USB hub tidak di-suspend, dan USB 3 link power management mati.

**Berisiko: mengurangi keamanan** (tidak ikut "Terapkan semua"):
- **Matikan mitigasi Spectre dan Meltdown** (baru, perlu restart): `FeatureSettingsOverride` dan `FeatureSettingsOverrideMask` = 3. Paling terasa di Intel generasi 9 ke bawah; di CPU baru hasilnya kecil.
- **Matikan Memory Integrity (HVCI)** (perlu restart): bisa menambah beberapa persen FPS, tetapi melemahkan perlindungan Windows terhadap driver dan malware tingkat kernel. Beberapa anti-cheat (Vanguard, FACEIT) bisa meminta fitur ini menyala. Bila dikunci UEFI atau kebijakan organisasi, pengaturan ini tidak berpengaruh; cek hasilnya di Windows Security → Device security → Core isolation.

Defender, Firewall, dan mitigasi CPU tidak disentuh Pacu.

### BIOS & Hardware (baru di v0.8)
Halaman yang **hanya membaca** dan memberi tahu pengaturan BIOS apa yang perlu kamu ubah sendiri. Pacu tidak pernah menulis ke BIOS atau NVRAM.
- Membaca tabel SMBIOS (`GetSystemFirmwareTable`): motherboard, versi dan tanggal BIOS, CPU, dan tiap keping RAM (kecepatan yang dipakai, kecepatan maksimal, merek, kode part).
- Status Resizable BAR dari `nvidia-smi` (jendela BAR1 256 MB = mati, setara VRAM = aktif), mode boot UEFI/Legacy, Secure Boot, dan Memory Integrity.
- Rekomendasi bertingkat (Penting, Saran, Opsional) dengan letak menu untuk ASUS, MSI, Gigabyte, dan ASRock: XMP/EXPO, RAM single channel, Resizable BAR + Above 4G Decoding, CSM/UEFI (dengan peringatan mbr2gpt), update BIOS, PBO/Curve Optimizer untuk Ryzen, C-states, dan HVCI.
- **Salin laporan** menyalin ringkasan teks untuk dikirim ke Claude atau forum.

### RAM Ekstrem (baru)
Halaman untuk menekan RAM terpakai saat tidak bermain.
- **Rampingkan sekarang** mengosongkan working set semua proses lalu menulis halaman termodifikasi ke pagefile, sehingga RAM terpakai langsung turun. Hasilnya (sebelum dan sesudah) ditampilkan dan dicatat di Aktivitas.
- **Rampingkan otomatis** (nyala bawaan): tiap 5 menit bila RAM terpakai di atas 1,5 GB, dan tidak pernah selama Mode Game supaya game tidak tersendat. Interval dan batasnya bisa diubah.
- **Layanan dan aplikasi latar**: SysMain, pengindeksan Windows Search, telemetri (DiagTrack), layanan jarang dipakai (peta, lokasi, mode demo, Program Compatibility Assistant, pelaporan error), penggabungan layanan svchost (baru, dari AtlasOS), Print Spooler, Widgets dan Copilot, Edge di latar, dan aplikasi Store di latar. Semua dicadangkan dan bisa dikembalikan; layanan berlaku setelah restart.
- **Aplikasi saat Windows menyala**: daftar aplikasi startup (registry Run, folder Startup, dan aplikasi Store seperti Teams) dengan sakelar nyala/mati. Caranya sama dengan tab Startup di Task Manager, jadi entri aslinya tidak dihapus.
- **Discord** (baru di v0.7): penghemat RAM khusus Discord, PTB, Canary, dan Vesktop.
  - *Rampingkan berkala* (bawaan tiap 1 menit, juga saat bermain): mengosongkan working set semua proses Discord selama jendelanya tidak di depan. Angka RAM Discord langsung turun, lalu naik pelan-pelan sesuai pemakaian; halaman yang dipakai lagi dibaca balik dari RAM standby.
  - *Redam saat Mode Game*: prioritas CPU di bawah normal, I/O rendah, dan Efficiency Mode selama game jalan; dikembalikan saat Mode Game mati atau saat jendela Discord dibuka.
  - *Batas keras RAM per proses* (mati bawaan; 150, 250, 400, atau 600 MB): Windows tidak mengizinkan proses Discord memakai RAM fisik melebihi angka itu (`SetProcessWorkingSetSizeEx` dengan `QUOTA_LIMITS_HARDWS_MAX_ENABLE`). Batas dilepas selama jendela Discord di depan dan dipasang lagi setelahnya.
  - *Pengaturan di dalam Discord*: matikan akselerasi hardware dan "buka saat Windows menyala" lewat `settings.json` Discord. Discord ditutup, file dicadangkan ke `settings.json.pacu-bak`, lalu Discord dibuka lagi.
- **Pemakan RAM terbesar**: 10 aplikasi dengan working set terbesar, digabung per nama.
- Target 1 GB biasanya tidak tercapai: Windows 11 sendiri (kernel, Explorer, Defender, driver GPU, layanan inti) umumnya sudah memakai lebih dari itu, dan Pacu tidak membuang Defender atau komponen inti. Setelah dirampingkan, angkanya naik lagi pelan-pelan saat aplikasi dipakai; halaman yang dibutuhkan dibaca balik dari RAM standby, bukan dari disk.

### Penyeimbang CPU (setara ProBalance)
- Meredam sementara proses latar yang memakan CPU berlebihan, lalu memulihkannya saat sudah tenang.
- Ambang otomatis mengikuti jumlah inti CPU. Contoh untuk 8 thread: CPU total di atas 11% dan satu proses di atas 8% selama 0,9 detik. Proses dipulihkan bila CPU-nya turun di bawah 4% dan sudah diredam minimal 4,2 detik.
- Aplikasi di depan, anak prosesnya, game, layanan Windows, dan proses sistem penting tidak pernah disentuh.
- Prioritas hanya dikembalikan bila belum kamu ubah sendiri.
- Opsi tambahan: turunkan ke Idle, turunkan prioritas disk, pakai Efficiency Mode, jeda saat PC tidak dipakai, daftar pengecualian, atau ambang manual.

### Proses dan aturan
- Daftar proses dengan CPU, RAM, prioritas, dan status (Game / Diredam / Aturan); bisa dicari dan diurutkan.
- Klik kanan sebuah proses untuk mengubah prioritas CPU/I/O/GPU sekarang, membuat aturan permanen, menjadikannya game, menandai "jangan pernah diredam", atau menghentikannya.
- **Aturan** per aplikasi diterapkan setiap kali aplikasi itu berjalan: prioritas CPU, I/O, GPU, affinity, Efficiency Mode, dan "jangan diredam". Pola wildcard didukung, misalnya `*\steamapps\common\*`.

### Aktivitas
Riwayat semua tindakan Pacu yang bisa difilter. Log juga disimpan di `%APPDATA%\Pacu\pacu.log`.

## Hasil mempelajari RivaTuner Statistics Server 7.3.7
Installer RTSS dibongkar dan dipelajari, tanpa menyalin kode atau asetnya:
- **FPS dan overlay RTSS berasal dari hook yang disuntikkan ke setiap game** (`RTSSHooks64.dll`). Pacu sengaja tidak meniru ini: menyuntik DLL ke game berisiko ban anti-cheat (VAC, EAC, Vanguard), sedangkan RTSS sudah masuk daftar putih banyak anti-cheat. Karena itu Pacu bekerja sama dengan RTSS alih-alih menggantikannya.
- RTSS menyediakan **shared memory `RTSSSharedMemoryV2`** yang terdokumentasi di SDK-nya: daftar aplikasi 3D beserta FPS dan frame time, aplikasi yang sedang di depan, dan 8 slot teks OSD yang bisa dipakai aplikasi lain (cara yang sama dipakai HWiNFO dan AIDA64). Pacu membaca FPS dari sana dan menulis teksnya ke slot pertama yang kosong setelah slot 0, yang dibiarkan untuk Afterburner.
- **RTSS sendiri tidak mengukur suhu CPU.** Untuk itu RTSS 7.3.7 membawa LibreHardwareMonitor yang memakai driver WinRing0. WinRing0 kini ditandai Windows Defender sebagai alat berbahaya karena celah keamanannya, jadi Pacu memakai penggantinya, PawnIO, yang bertanda tangan dan hanya mengizinkan pembacaan register sensor.
- Suhu CPU dibaca langsung dari register prosesor: Intel lewat `IA32_PACKAGE_THERM_STATUS` (0x1B1) dikurangi dari TjMax (0x1A2), AMD Ryzen lewat register SMN `THM_TCON_CUR_TMP` (0x59800). Daya paket dihitung dari penghitung energi RAPL (Intel 0x611, AMD 0xC001029B).

## Kenapa lebih ringan dari Process Lasso
- Satu proses saja: mesin, UI, dan tray digabung. Process Lasso memakai beberapa proses (service governor, GUI, agen sesi).
- Daftar proses dibaca sekali per detik lewat satu panggilan `NtQuerySystemInformation`, tanpa membuka handle ke setiap proses.
- UI hanya digambar ulang sekali per detik. Saat disembunyikan ke tray, UI berhenti digambar sama sekali.
- Ditulis dengan Rust tanpa .NET atau runtime lain.

## Batasan yang perlu diketahui
- Game dengan anti-cheat tingkat kernel (misalnya Vanguard, beberapa versi EAC/BattlEye) bisa menolak perubahan prioritas. Pacu mencatatnya di Aktivitas dan tetap menjalankan optimasi lain (power plan, timer, peredaman aplikasi latar).
- Di Windows 11, resolusi timer 0,5 ms dari Pacu baru berlaku untuk game setelah **Optimasi Windows → Izinkan resolusi timer global** diterapkan dan PC direstart.
- Membekukan aplikasi yang sedang dipakai (misalnya Discord saat voice chat) akan membuatnya berhenti. Hapus aplikasi itu dari daftar pembekuan.
- UI membutuhkan OpenGL 2.0. Semua driver GPU modern mendukungnya; VM tanpa akselerasi GPU mungkin tidak bisa membuka jendelanya.
- Prioritas Realtime untuk game tidak disarankan karena bisa membuat input mouse/keyboard tersendat.
- Overlay jendela Pacu tidak terlihat di game Fullscreen eksklusif. Untuk itu Pacu memakai RTSS: dengan **Jalankan RTSS otomatis** (nyala bawaan), Pacu membuka RTSS bila sudah terpasang, lalu RTSS yang menggambar overlay Pacu di dalam game. Buka game setelah RTSS berjalan; game yang sudah terbuka perlu dibuka ulang.
- Profil driver NVIDIA dan overclock VRAM belum diuji di GPU sungguhan. Urutan panggilan NVAPI, ukuran struct, dan alur pencarian sudah diuji di Wine dengan driver NVIDIA tiruan.
- Undervolt AMD belum diuji di kartu AMD sungguhan. Alur ADLX (membaca kartu, menulis tegangan/frekuensi maksimum, sensor, reset) sudah diuji di Wine dengan driver RX 6700 XT tiruan. Kirim Laporan diagnostik dari tab Log & Diagnostik bila ada yang tidak berjalan.
- Monitor dan overlay belum diuji di PC sungguhan: integrasi RTSS, overlay jendela, dan sensor GPU sudah diuji di Wine dengan RTSS dan driver NVIDIA tiruan, tetapi pembacaan suhu CPU lewat PawnIO baru akan terbukti di PC kamu.

## Pengaturan
Disimpan di `%APPDATA%\Pacu\config.json` dan bisa disalin ke PC lain.

## Build dari source
Butuh Rust 1.87+.

```
# Di Windows
cargo build --release

# Dari Linux (cross-compile)
rustup target add x86_64-pc-windows-gnu
sudo apt install gcc-mingw-w64-x86-64
cargo build --release --target x86_64-pc-windows-gnu
```

Installer dibuat dengan NSIS (`sudo apt install nsis`) dari `installer/pacu.nsi`; lihat langkah "Kemas" di `.github/workflows/release.yml`.

Di Linux, `cargo run` menjalankan UI dengan data proses simulasi. `cargo test` menguji logika penyeimbang dan pencocokan pola.

## Struktur kode
- `src/balancer.rs` — logika penyeimbang CPU (murni, ada unit test)
- `src/engine.rs` — mesin: sampling, aturan, Mode Game, penerapan tindakan
- `src/sys/windows.rs` — panggilan API Windows
- `src/sys/sim.rs` — simulasi untuk pengembangan di luar Windows
- `src/ui.rs` — antarmuka egui
- `src/config.rs` — pengaturan dan nilai default
- `src/presets.rs` — preset game
- `src/tweaks.rs` — optimasi Windows (termasuk layanan untuk RAM Ekstrem) dan cadangannya
- `src/startup.rs` — daftar dan sakelar aplikasi startup
- `src/discord.rs` — penghemat RAM Discord: nama proses dan pengaturan `settings.json`
- `src/nvprofile.rs` — profil driver NVIDIA per game (NvAPI_DRS)
- `src/volt/vram.rs` — overclock VRAM otomatis (uji bandwidth D3D11 dan pencarian offset)
- `src/ui/ram_page.rs` — halaman RAM Ekstrem
- `src/bios.rs` — pembaca SMBIOS, status ReBAR, dan rekomendasi BIOS (murni, ada unit test); halamannya di `src/ui/bios_page.rs`
- `src/fps.rs` — pengukur FPS dan benchmark (PresentMon)
- `src/monitor/` — monitor sensor (`cpu.rs` lewat PDH dan PawnIO), integrasi RTSS (`rtss.rs`), isi overlay (`osd.rs`), dan jendela overlay (`overlay.rs`)
- `src/volt/` — Pacu Volt: kurva V/F NVAPI, backend AMD ADLX (`gpu/adlx.rs`, model slider di `gpu/amdmodel.rs`), pencarian titik undervolt, pemeriksa error D3D11, dan halamannya (`panel.rs`)
- `src/ui/monitor_page.rs` — halaman Monitor & Overlay

## Lisensi pihak ketiga
- Beberapa tweak di halaman Optimasi Windows, RAM Ekstrem, dan power plan terinspirasi playbook AtlasOS (© tim AtlasOS, GPL-3.0). Yang diambil hanya idenya (nilai registry dan setelan power plan yang terdokumentasi); kodenya ditulis ulang, tidak ada file AtlasOS yang disalin atau dibundel.
- Fluent System Icons © Microsoft, lisensi MIT (subset di `res/fonts/FluentIcons.ttf`, lisensi di `res/fonts/FluentSystemIcons-LICENSE.txt`).
- PresentMon 2.6.0 © Intel Corporation, lisensi MIT. Teks lisensinya ada di `res/PresentMon-LICENSE.txt`.
- Modul PawnIO `IntelMSR.bin` dan `AMDFamily17.bin` (bertanda tangan, dari proyek PawnIO.Modules), lisensi LGPL 2.1, teks lisensinya di `res/pawnio/LICENSE-PawnIO.Modules.txt`. Driver PawnIO sendiri tidak pernah dibundel; dipasang terpisah oleh pengguna.
- Tata letak interface ADLX diambil dari header ADLX SDK (© Advanced Micro Devices, lisensi MIT). Tidak ada kode atau DLL AMD yang dibundel; `amdadlx64.dll` dimuat dari driver yang terpasang.
- NVAPI SDK © NVIDIA Corporation, lisensi MIT: ID fungsi NvAPI_DRS_*, ID setting driver, dan tata letak struct Pstates20 diambil dari headernya. Tidak ada kode NVIDIA yang dibundel; `nvapi64.dll` dimuat dari driver.
