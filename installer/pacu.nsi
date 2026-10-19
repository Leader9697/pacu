; Installer Pacu (NSIS 3). Dibangun di GitHub Actions dengan:
;   makensis -DVERSION=0.6.0 -DSRC=<folder berisi Pacu.exe, pacu.ico, dan file lisensi> -DOUT=<Pacu-Setup.exe> installer/pacu.nsi
; Memasang ke Program Files\Pacu, membuat shortcut Start Menu dan desktop, dan mendaftarkan Pacu di
; Settings > Apps supaya bisa di-uninstall. Pembaruan dari dalam Pacu tetap mengganti Pacu.exe di
; folder ini (Pacu selalu berjalan sebagai administrator) lalu menyesuaikan DisplayVersion.

Unicode true
SetCompressor /SOLID lzma
ManifestDPIAware true
RequestExecutionLevel admin

!ifndef VERSION
  !error "Berikan -DVERSION=x.y.z"
!endif
!ifndef SRC
  !error "Berikan -DSRC=<folder file Pacu>"
!endif
!ifndef OUT
  !define OUT "Pacu-Setup.exe"
!endif

!define APP "Pacu"
!define PUBLISHER "Leader9697"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Pacu"

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

Name "${APP} ${VERSION}"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\${APP}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "${APP}"
VIAddVersionKey /LANG=1033 "FileDescription" "Installer ${APP}"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "MIT"

!define MUI_ICON "${SRC}\pacu.ico"
!define MUI_UNICON "${SRC}\pacu.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\Pacu.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Buka Pacu sekarang"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Indonesian"

; Pacu yang sedang berjalan mengunci Pacu.exe; tutup dulu sebelum menimpa atau menghapus.
!macro CLOSE_PACU
  nsExec::Exec 'taskkill /F /IM Pacu.exe'
  Pop $0
  Sleep 800
!macroend

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Pacu membutuhkan Windows 64-bit." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "Pacu (wajib)" SecMain
  SectionIn RO
  !insertmacro CLOSE_PACU
  SetOutPath "$INSTDIR"
  ; Sisa pembaruan otomatis yang belum sempat dibersihkan.
  Delete "$INSTDIR\Pacu.exe.old"
  Delete "$INSTDIR\Pacu.exe.new"
  File "${SRC}\Pacu.exe"
  File "${SRC}\README.md"
  File "${SRC}\CHANGELOG.md"
  File "${SRC}\PresentMon-LICENSE.txt"
  File "${SRC}\LICENSE-PawnIO.Modules.txt"
  File "${SRC}\FluentSystemIcons-LICENSE.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  SetShellVarContext all
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\Pacu.exe" "" "$INSTDIR\Pacu.exe" 0

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\Pacu.exe,0"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "URLInfoAbout" "https://github.com/Leader9697/pacu"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Shortcut di desktop" SecDesktop
  SetShellVarContext all
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\Pacu.exe" "" "$INSTDIR\Pacu.exe" 0
SectionEnd

Section "Uninstall"
  !insertmacro CLOSE_PACU
  ; Tugas Task Scheduler yang dibuat Pacu (jalan saat login dan undervolt saat login).
  nsExec::Exec 'schtasks /Delete /F /TN "Pacu Game Booster"'
  Pop $0
  nsExec::Exec 'schtasks /Delete /F /TN "Pacu Volt Undervolt"'
  Pop $0

  Delete "$INSTDIR\Pacu.exe"
  Delete "$INSTDIR\Pacu.exe.old"
  Delete "$INSTDIR\Pacu.exe.new"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\PresentMon-LICENSE.txt"
  Delete "$INSTDIR\LICENSE-PawnIO.Modules.txt"
  Delete "$INSTDIR\FluentSystemIcons-LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  SetShellVarContext all
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKLM "${UNINST_KEY}"

  ; Pengaturan, profil undervolt, dan cadangan tweak ada di %APPDATA%. Defaultnya disimpan supaya
  ; memasang ulang tidak mengulang dari nol.
  SetShellVarContext current
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Hapus juga pengaturan Pacu dan profil undervolt?$\r$\n$\r$\nTweak Windows yang sudah diterapkan tidak dikembalikan otomatis. Kembalikan dulu dari dalam Pacu bila perlu." /SD IDNO IDNO keep
    RMDir /r "$APPDATA\Pacu"
    RMDir /r "$APPDATA\PacuVolt"
  keep:
SectionEnd
