; Preserve registration during updater-driven reinstalls.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-login-tasks' $0
    ${If} $0 <> 0
      MessageBox MB_OK|MB_ICONSTOP "Could not remove the Mihomo Client login task. Disable Start on login in the app, then uninstall again."
      Abort
    ${EndIf}
  ${EndIf}
!macroend
