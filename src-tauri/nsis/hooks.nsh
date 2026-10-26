; Overstap van de oude productnaam ("OpenAEC Storage", versies t/m 0.4) naar "OpenAEC Desktop App".
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
; Vaste CLSID van het item "OpenAEC" in het navigatiepaneel (gelijk aan navigatie::CLSID).
!define NAVIGATIE_CLSID "{A1046B79-4B13-4993-B03B-4D94B67DDB65}"
; Let op: dit bestand wordt bovenaan het installatiescript ingevoegd, vóór de defines van Tauri (MANUFACTURER,
; PRODUCTNAME, ...). Die mogen daarom alleen binnen de macro's gebruikt worden.

Var OudeMap

; Het moderne rechtermuismenu van Windows 11 (menupakket.rs, crates/openaec-shellmenu): een DLL die de Verkenner nog
; geladen heeft, kan niet overschreven worden, wel hernoemd. Hernoemde exemplaren gaan weg zodra de Verkenner ze heeft
; losgelaten (bij een volgende installatie of het verwijderen).
!macro OPENAEC_SHELLMENU_VRIJ ARCH
  Delete "$INSTDIR\shellmenu\${ARCH}\*.oud"
  ${If} ${FileExists} "$INSTDIR\shellmenu\${ARCH}\openaec_shellmenu.dll"
    System::Call 'kernel32::GetTickCount() i .R8'
    Rename "$INSTDIR\shellmenu\${ARCH}\openaec_shellmenu.dll" "$INSTDIR\shellmenu\${ARCH}\openaec_shellmenu-$R8.oud"
    Delete "$INSTDIR\shellmenu\${ARCH}\openaec_shellmenu-$R8.oud"
  ${EndIf}
!macroend

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
  !insertmacro OPENAEC_SHELLMENU_VRIJ "x64"
  !insertmacro OPENAEC_SHELLMENU_VRIJ "arm64"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Het moderne rechtermuismenu van Windows 11: het ondertekende pakket voor deze gebruiker registreren, met de
  ; installatiemap als external location (de app doet hetzelfde bij het starten als het ontbreekt). Lukt het niet,
  ; dan blijft het klassieke menu.
  ${If} ${FileExists} "$INSTDIR\shellmenu\*.msix"
    ; ExecWait (ingebouwd) in plaats van de plugin nsExec: die zit niet ondertekend in de installer. De app heeft
    ; geen console, dus er verschijnt geen venster.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --menupakket registreer' $R9
    DetailPrint "Rechtermuismenu van Windows 11: $R9"
  ${EndIf}
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

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; Het pakket van het moderne rechtermuismenu afmelden, zolang de app er nog is (menupakket.rs).
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --menupakket afmelden' $R9
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
    ; Het item "OpenAEC" in het navigatiepaneel voor losse koppelingen (navigatie.rs, vaste CLSID), en de map met
    ; mapsnelkoppelingen die het toont. Daarna het navigatiepaneel laten verversen (SHCNE_ASSOCCHANGED).
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\${NAVIGATIE_CLSID}"
    DeleteRegKey HKCU "Software\Classes\CLSID\${NAVIGATIE_CLSID}"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel" "${NAVIGATIE_CLSID}"
    RMDir /r "$LOCALAPPDATA\OpenAEC Storage\Verkenner"
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
    ; "Versturen met OpenAEC" op elk bestand en elke map (rechtermuis.rs).
    DeleteRegKey HKCU "Software\Classes\*\shell\OpenAECVersturen"
    DeleteRegKey HKCU "Software\Classes\Directory\shell\OpenAECVersturen"
    ; Hernoemde DLL's van het moderne menu die de Verkenner inmiddels heeft losgelaten.
    Delete "$INSTDIR\shellmenu\x64\*.oud"
    Delete "$INSTDIR\shellmenu\arm64\*.oud"
    RMDir "$INSTDIR\shellmenu\x64"
    RMDir "$INSTDIR\shellmenu\arm64"
    RMDir "$INSTDIR\shellmenu"
  ${EndIf}
!macroend
