; SPDX-License-Identifier: GPL-3.0-or-later
; Keenu's setup program: per-user, no administrator rights, data left alone on uninstall.
;
;   makensis -DVERSION=2.0.0 -DNUMERIC_VERSION=2.0.0.0 -DEXE=keenu.exe -DICON=keenu.ico -DLICENSE=LICENSE \
;            -DOUT=Keenu-2.0.0-windows-x64-setup.exe packaging/windows/installer.nsi
;
; /S installs without asking, for scripts.

Unicode true
!include "MUI2.nsh"
!include "x64.nsh"

Name "Keenu"
OutFile "${OUT}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\Keenu"
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Keenu" "InstallLocation"
SetCompressor /SOLID lzma
ManifestDPIAware true

VIProductVersion "${NUMERIC_VERSION}"
VIAddVersionKey "ProductName" "Keenu"
VIAddVersionKey "FileDescription" "Keenu Setup"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GPL-3.0-or-later. Based on Isle by zickrian."

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\keenu.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start Keenu"
!insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Keenu"

; A running Keenu holds its program file; asked to quit, it saves everything first.
!macro CloseKeenu
  FindWindow $0 "KeenuIslandWindow"
  IntCmp $0 0 +3
    SendMessage $0 16 0 0 /TIMEOUT=3000
    Sleep 800
!macroend

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "This version of Keenu is for 64-bit Windows."
    Abort
  ${EndIf}
FunctionEnd

Section "Keenu"
  !insertmacro CloseKeenu
  SetOutPath "$INSTDIR"
  File "/oname=keenu.exe" "${EXE}"
  File "/oname=LICENSE.txt" "${LICENSE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Keenu.lnk" "$INSTDIR\keenu.exe"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Keenu"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\keenu.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  !insertmacro CloseKeenu
  ExecWait '"$INSTDIR\keenu.exe" --agent-uninstall'
  ExecWait '"$INSTDIR\keenu.exe" --disable-startup'
  Delete "$INSTDIR\keenu.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Keenu.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; %LOCALAPPDATA%\Keenu stays: it is the user's. Settings, Delete my data, removes it.
SectionEnd
