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
  ; Never prompt during an unattended install: a modal here HANGS the update, because the old agent
  ; has already exited (its timer stopped) and nothing tracks until a human clicks. Unattended
  ; installs simply leave the existing choice untouched.
  ;
  ; This guard was written believing the auto-updater reinstalls with `/S`. It does not by default:
  ; tauri-plugin-updater's default Windows mode is `passive`, which passes `/P /R`, and `IfSilent`
  ; is only true for `/S`. So the prompt fired on every auto-update and sat behind whatever the
  ; employee was working in -- one report lost ~1.5h of tracked time to a dialog nobody saw.
  ; `plugins.updater.windows.installMode` is now pinned to "quiet" (`/S /R`) in tauri.conf.json, so
  ; this branch is finally reached. Keep the two together: loosening the install mode without
  ; teaching this hook about `/P` brings the hang straight back.
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
