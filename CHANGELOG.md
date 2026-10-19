# Catatan rilis

## v0.6.0
- Installer resmi **Pacu-Setup.exe**: memasang ke Program Files, membuat shortcut Start Menu dan desktop, dan mendaftarkan Pacu di Settings → Apps lengkap dengan uninstall. Uninstall juga menghapus tugas Task Scheduler milik Pacu dan menawarkan untuk menghapus pengaturan. `Pacu.exe` portable tetap tersedia.
- Pembaruan otomatis juga bekerja untuk Pacu yang dipasang lewat installer, dan versi di Settings → Apps ikut diperbarui.
- Tugas "Jalankan saat Windows menyala" otomatis diarahkan ke lokasi Pacu yang sedang dipakai (misalnya setelah pindah dari portable ke installer).
- Properti file Pacu.exe kini menampilkan versi yang benar.

## v0.5.0
- Optimasi Windows punya bagian baru **Ekstrem** dengan 9 tweak yang idenya diadaptasi dari AtlasOS: prioritas CPU untuk jendela aktif, fullscreen eksklusif asli tanpa Game Bar, kernel dikunci di RAM, Fault Tolerant Heap mati, NTFS lebih ringan, animasi mati, startup tanpa jeda, shutdown cepat, dan Fast Startup mati. Ada "Terapkan semua" sendiri dan setiap tweak bisa dikembalikan.
- Bagian Berisiko: tombol baru untuk mematikan mitigasi Spectre dan Meltdown.
- RAM Ekstrem: gabungkan layanan dalam sedikit svchost (puluhan proses dan ratusan MB RAM berkurang).
- Power plan "Pacu Performa" lebih agresif: tanpa throttle state CPU, NVMe tidak pernah idle, USB hub dan USB 3 tidak hemat daya.

## v0.4.0
- Pembaruan otomatis: Pacu memeriksa rilis baru di GitHub saat menyala dan tiap 6 jam, mengunduh lewat HTTPS, mencocokkan SHA-256, lalu memasang versi baru setelah kamu menekan "Mulai ulang".
- Pengaturan "Bangun dari sleep": pilih perangkat (mouse, keyboard, kartu jaringan, USB hub) yang boleh membangunkan PC. Pilihan dipasang ulang setiap Pacu menyala.
- Beranda Undervolt menampilkan status undervolt dan overclock VRAM yang dibaca langsung dari driver; "Pasang semua" memasang VRAM dulu lalu undervolt.
- Tampilan baru bergaya Windows 11 (Fluent) dan tabel sensor ala HWiNFO.
