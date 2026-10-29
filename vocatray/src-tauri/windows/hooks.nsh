; VocaTray 설치 프로그램 훅 (tauri.conf.json › bundle.windows.nsis.installerHooks)

; 트레이에 사는 앱이라 시작 메뉴·바탕 화면 바로 가기를 만들지 않는다.
; 기본 스크립트의 바로 가기 함수는 $NoShortcutMode가 1이면 건너뛴다.
; 마지막 화면의 "바탕 화면 바로 가기 만들기"는 체크를 끈 채로 보여 준다 (직접 체크한 사람만 만든다).
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

!macro NSIS_HOOK_PREINSTALL
  StrCpy $NoShortcutMode 1
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; 마지막 화면에서 직접 체크하면 바탕 화면 바로 가기를 만들 수 있게 되돌린다
  StrCpy $NoShortcutMode 0
  ; 시작 시 자동 실행은 기본으로 켠다 — 처음 설치할 때(이 PC에 학습 기록이 없을 때)만.
  ; 업데이트할 때 사용자가 트레이 메뉴에서 꺼 둔 설정을 되살리지 않게.
  ; 덮어 설치하면서 옛 버전 제거가 지운 자동 실행(아래 PREUNINSTALL)은 되살린다.
  Push $0
  ReadRegStr $0 HKCU "Software\VocaTray" "RestoreAutostart"
  ${If} $0 == "1"
  ${OrIfNot} ${FileExists} "$APPDATA\com.vocatray.desktop\device.json"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "VocaTray" '"$INSTDIR\vocatray.exe"'
  ${EndIf}
  DeleteRegKey HKCU "Software\VocaTray"
  Pop $0
!macroend

; 설치 파일로 덮어 설치하면 기본 스크립트가 옛 버전 제거를 먼저 돌리고, 그 제거가 자동 실행 항목을 지운다
; (자동 업데이트(/UPDATE) 때만 빼고). 켜져 있었으면 표시를 남겨 POSTINSTALL에서 되살린다.
; 정말 제거할 때도 표시가 남지만 다음에 설치할 때 자동 실행이 원래대로 켜질 뿐이다.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    Push $0
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "VocaTray"
    ${If} $0 != ""
      WriteRegStr HKCU "Software\VocaTray" "RestoreAutostart" "1"
    ${EndIf}
    Pop $0
  ${EndIf}
!macroend
