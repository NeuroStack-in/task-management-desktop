; WorkPulse NSIS installer hooks (Tauri v2 `bundle.windows.nsis.installerHooks`).
;
; Asks the user, during installation, whether the agent should launch when they sign in to Windows.
; We DON'T write the autostart registry key here — the app's autostart plugin owns that, so the
; installer only records the user's choice in `%APPDATA%\com.workpulse.agent\autostart.choice`
; (1 = yes, 0 = no). On first launch the app reads it and calls the plugin's enable(), keeping the
; OS entry and `get_auto_start` perfectly consistent. Deleting `autostart.initialized` forces the app
; to re-apply the choice, so re-running the installer with a different answer actually takes effect.
;
; $APPDATA here is the installing user's roaming AppData, which is exactly where Tauri's
; app_config_dir() resolves for identifier `com.workpulse.agent` — so the paths line up.

!macro NSIS_HOOK_POSTINSTALL
  ; Never prompt during a silent install — that's how the auto-updater reinstalls (`/S`), and a
  ; modal here would hang the update. Silent installs simply leave the existing choice untouched.
  IfSilent autostart_done
    MessageBox MB_YESNO|MB_ICONQUESTION "Launch WorkPulse automatically when you sign in to Windows?$\n$\nRecommended so activity tracking starts with your session." IDNO autostart_no
      StrCpy $0 "1"
      Goto autostart_write
    autostart_no:
      StrCpy $0 "0"
    autostart_write:
      CreateDirectory "$APPDATA\com.workpulse.agent"
      Delete "$APPDATA\com.workpulse.agent\autostart.initialized"
      ClearErrors
      FileOpen $1 "$APPDATA\com.workpulse.agent\autostart.choice" w
      IfErrors autostart_done
      FileWrite $1 $0
      FileClose $1
  autostart_done:
!macroend
