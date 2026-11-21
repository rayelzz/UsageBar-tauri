; Keep the final action explicit. Installing UsageBar must not look like merely
; launching a portable executable.
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

Var UsageBarAutostartCheckbox
Var UsageBarAutostartState
Var UsageBarOptionsVisited

; Tauri includes installer hooks before its built-in pages. This options page
; therefore becomes the first page after the optional language selector.
Page custom UsageBarOptionsCreate UsageBarOptionsLeave

Function UsageBarOptionsCreate
  ; Updater and unattended installs must preserve the user's current setting.
  IfSilent usagebar_options_skip
  ClearErrors
  ${GetOptions} $CMDLINE "/P" $0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}
  ClearErrors
  ${GetOptions} $CMDLINE "/UPDATE" $0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}

  StrCpy $0 "Installation options"
  StrCpy $1 "Choose how UsageBar should start. You can change this later in Settings."
  StrCpy $2 "Start UsageBar automatically with Windows"
  StrCpy $3 "The setup wizard will ask about a desktop shortcut and launching UsageBar on its final page."

  ; The hook is included before MUI declares symbolic language constants.
  ; Compare the stable Windows LANGIDs directly: ru-RU=1049, zh-CN=2052.
  ${If} $LANGUAGE == 1049
    StrCpy $0 "Параметры установки"
    StrCpy $1 "Выберите способ запуска UsageBar. Позже это можно изменить в настройках."
    StrCpy $2 "Автоматически запускать UsageBar вместе с Windows"
    StrCpy $3 "На последнем шаге мастер отдельно предложит ярлык на рабочем столе и запуск UsageBar."
  ${ElseIf} $LANGUAGE == 2052
    StrCpy $0 "安装选项"
    StrCpy $1 "选择 UsageBar 的启动方式。稍后可在设置中更改。"
    StrCpy $2 "登录 Windows 时自动启动 UsageBar"
    StrCpy $3 "安装向导最后一页会另外询问是否创建桌面快捷方式以及是否立即启动 UsageBar。"
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "$0" "$1"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateCheckbox} 0 18u 100% 18u "$2"
  Pop $UsageBarAutostartCheckbox

  ; Preserve an installed user's choice. Enable by default only on a fresh
  ; install, because an edge widget is useful only when it remains available.
  ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "UsageBar"
  ${If} $1 != ""
    ${NSD_Check} $UsageBarAutostartCheckbox
  ${Else}
    ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UsageBar" "DisplayName"
    ${If} $1 == ""
      ${NSD_Check} $UsageBarAutostartCheckbox
    ${EndIf}
  ${EndIf}

  ${NSD_CreateLabel} 0 58u 100% 34u "$3"
  Pop $1
  StrCpy $UsageBarOptionsVisited 1
  nsDialogs::Show
  Return

  usagebar_options_skip:
    Abort
FunctionEnd

Function UsageBarOptionsLeave
  ${NSD_GetState} $UsageBarAutostartCheckbox $UsageBarAutostartState
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  ${If} $UsageBarOptionsVisited == 1
    ${If} $UsageBarAutostartState == ${BST_CHECKED}
      WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "UsageBar" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "UsageBar"
    ${Else}
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "UsageBar"
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "UsageBar"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "UsageBar"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "usagebar"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "UsageBar"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "usagebar"
!macroend
