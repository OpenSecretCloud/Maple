; Install the staged Windows runtime DLLs next to maple.exe.
;
; Why a hook instead of bundle.resources: in Tauri 2.11 a `resources` map
; supplied via the platform config (tauri.windows.conf.json) gets the DLLs into
; the installer payload but NOT into the NSIS install file-list, so they never
; land in the install dir (verified empirically). This hook copies them
; explicitly, independent of that plumbing.
;
; Path math: ${MAINBINARYSRCPATH} is the absolute build path to the main exe,
;   <src-tauri>\target\release\maple.exe
; so THREE "..\" segments climb maple.exe -> release -> target -> <src-tauri>,
; then into resources\windows where CI (and local builds) stage the DLLs.
; `File` embeds them at makensis compile time, so they must exist then (the
; build's "Stage Windows runtime DLLs" step guarantees that) and a missing file
; fails the build loudly rather than shipping a broken installer.
!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File "/oname=onnxruntime.dll"    "${MAINBINARYSRCPATH}\..\..\..\resources\windows\onnxruntime.dll"
  File "/oname=VCRUNTIME140.dll"   "${MAINBINARYSRCPATH}\..\..\..\resources\windows\VCRUNTIME140.dll"
  File "/oname=VCRUNTIME140_1.dll" "${MAINBINARYSRCPATH}\..\..\..\resources\windows\VCRUNTIME140_1.dll"
  File "/oname=MSVCP140.dll"       "${MAINBINARYSRCPATH}\..\..\..\resources\windows\MSVCP140.dll"
  File "/oname=MSVCP140_1.dll"     "${MAINBINARYSRCPATH}\..\..\..\resources\windows\MSVCP140_1.dll"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\VCRUNTIME140.dll"
  Delete "$INSTDIR\VCRUNTIME140_1.dll"
  Delete "$INSTDIR\MSVCP140.dll"
  Delete "$INSTDIR\MSVCP140_1.dll"
!macroend
