; Overstap van de oude productnaam ("OpenAEC Storage", versies t/m 0.4) naar "OpenAEC SuperCloud Desktop".
;
; De NSIS-installer van Tauri leidt de installatiemap ($LOCALAPPDATA\<productnaam>), de uninstall-sleutel
; (HKCU\...\Uninstall\<productnaam>) en de sleutel met de installatiemap (HKCU\Software\<uitgever>\<productnaam>) af
; van de productnaam. Zonder deze hooks zou een update vanuit de oude app de nieuwe naast de oude installeren: twee
; apps, twee items in "Apps en onderdelen" en een oude snelkoppeling naar een exe die niet meer wordt bijgewerkt.
;
; Daarom: staat er een installatie onder de oude naam (en nog niet onder de nieuwe), dan installeert deze installer
; over die map heen. Het exe-pad blijft dan gelijk, zodat het rechtermuismenu, de pictogrammen van de sync-roots,
; vastgemaakte snelkoppelingen en de autostart-regel (vaste waardenaam, zie lib.rs) blijven kloppen. Na het
; installeren worden de registerregels van de oude naam opgeruimd en hernoemt hij de snelkoppelingen.

!define OUDE_NAAM "OpenAEC Storage"
!define OUDE_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${OUDE_NAAM}"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define STARTUPAPPROVEDKEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
; Let op: dit bestand wordt bovenaan het installatiescript ingevoegd, vóór de defines van Tauri (MANUFACTURER,
; PRODUCTNAME, ...). Die mogen daarom alleen binnen de macro's gebruikt worden.

Var OudeMap

!macro NSIS_HOOK_PREINSTALL
  StrCpy $OudeMap ""
  ; Alleen als er onder de nieuwe naam nog niets geïnstalleerd is en de gebruiker de standaardmap niet zelf
  ; veranderd heeft.
  ReadRegStr $R9 SHCTX "${MANUPRODUCTKEY}" ""
  ${If} $R9 == ""
  ${AndIf} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    ReadRegStr $R9 SHCTX "Software\${MANUFACTURER}\${OUDE_NAAM}" ""
    ${If} $R9 != ""
    ${AndIf} ${FileExists} "$R9\${MAINBINARYNAME}.exe"
      StrCpy $OudeMap $R9
      StrCpy $INSTDIR $R9
      SetOutPath $INSTDIR
      ; De lege map die SetOutPath zojuist voor de nieuwe naam aanmaakte.
      RMDir "$LOCALAPPDATA\${PRODUCTNAME}"
      DetailPrint "Bestaande installatie in $INSTDIR wordt bijgewerkt"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $OudeMap != ""
    ; "Apps en onderdelen" toont voortaan alleen de nieuwe naam (de nieuwe sleutel is net geschreven).
    DeleteRegKey SHCTX "${OUDE_UNINSTKEY}"
    DeleteRegKey SHCTX "Software\${MANUFACTURER}\${OUDE_NAAM}"
    ; Snelkoppelingen naar deze exe krijgen de nieuwe naam; bij een update maakt Tauri zelf geen nieuwe aan.
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${OUDE_NAAM}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $R9
    ${If} $R9 = 1
      ${If} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
        Delete "$SMPROGRAMS\${OUDE_NAAM}.lnk"
      ${Else}
        Rename "$SMPROGRAMS\${OUDE_NAAM}.lnk" "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      ${EndIf}
    ${EndIf}
    !insertmacro IsShortcutTarget "$DESKTOP\${OUDE_NAAM}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $R9
    ${If} $R9 = 1
      ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
        Delete "$DESKTOP\${OUDE_NAAM}.lnk"
      ${Else}
        Rename "$DESKTOP\${OUDE_NAAM}.lnk" "$DESKTOP\${PRODUCTNAME}.lnk"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; De autostart-regel heeft een vaste naam (de oude productnaam); Tauri ruimt alleen de regel met de huidige
    ; productnaam op.
    DeleteRegValue HKCU "${RUNKEY}" "${OUDE_NAAM}"
    DeleteRegValue HKCU "${STARTUPAPPROVEDKEY}" "${OUDE_NAAM}"
    ; Het rechtermuismenu in de Verkenner (verkenner.rs) wijst naar de verwijderde exe.
    DeleteRegKey HKCU "Software\Classes\*\shell\OpenAECStorage"
    DeleteRegKey HKCU "Software\Classes\Directory\shell\OpenAECStorage"
  ${EndIf}
!macroend
