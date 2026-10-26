//! "Versturen met OpenAEC" in het rechtermuismenu van de bestandsbeheerder, op elk bestand en elke map (niet alleen in
//! de gekoppelde mappen). Het item start de app met `--actie versturen <paden…>`; draait de app al, dan stuurt de
//! single-instance-plugin de paden door en verzamelt het venster "Versturen" ze tot één overdracht.
//!
//! - **Windows**: een eigen werkwoord in het hoofdmenu (HKCU\Software\Classes\*\shell en Directory\shell), met het
//!   app-pictogram en `MultiSelectModel=Player`: bij een selectie van meerdere items start de Verkenner de app per item,
//!   en de paden komen samen in één overdracht. Windows 11 toont zulke werkwoorden onder "Meer opties weergeven" (of
//!   direct, met het klassieke menu); het moderne menu vraagt een `IExplorerCommand` in een pakket met identiteit (zie
//!   de README). De uninstaller ruimt het werkwoord op (nsis/hooks.nsh).
//! - **Linux**: een script voor Nautilus (of een nautilus-python-extensie als die er is, dan staat het item direct in
//!   het menu), een service-menu voor Dolphin en een actie voor Nemo, in `~/.local/share`. Bij uitzetten in de
//!   instellingen gaan ze weg; bij het verwijderen van de .deb ruimt `linux/postrm` ze op.
//! - **macOS**: een snelle actie (`~/Library/Services/Versturen met OpenAEC.workflow`) voor de Finder.
//!
//! De app schrijft dit bij elke start opnieuw (alleen als er iets verandert), zodat het pad naar de app klopt na een
//! update of een verplaatste AppImage. Dit bestand staat los van de rest van de app (alleen std, winreg, dirs en
//! serde_json), zodat de rooktest (`examples/versturen_smoke.rs`) het kan meenemen.

use std::path::{Path, PathBuf};

/// Tekst van het item in het menu.
pub const LABEL: &str = "Versturen met OpenAEC";
/// De actie op de opdrachtregel (`--actie versturen <paden…>`).
pub const ACTIE: &str = "versturen";
/// Sleutelnaam van het werkwoord in het register (Windows).
pub const SLEUTEL: &str = "OpenAECVersturen";
/// De soorten waarop het werkwoord staat: alle bestanden en mappen.
pub const SOORTEN: [&str; 2] = ["*", "Directory"];

/// Eén waarde in het register, onder HKCU\Software\Classes; `naam` leeg = de standaardwaarde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registerwaarde {
    pub sleutel: String,
    pub naam: &'static str,
    pub waarde: String,
}

/// De registerwaarden voor het werkwoord `sleutel` met tekst `label` dat `exe` start (Windows).
pub fn windows_waarden(exe: &str, sleutel: &str, label: &str) -> Vec<Registerwaarde> {
    let mut uit = Vec::new();
    for soort in SOORTEN {
        let basis = format!("{soort}\\shell\\{sleutel}");
        let waarde = |naam, waarde: String| Registerwaarde { sleutel: basis.clone(), naam, waarde };
        uit.push(waarde("MUIVerb", label.to_string()));
        uit.push(waarde("Icon", format!("\"{exe}\",0")));
        // Ook bij een selectie van meerdere items (de app verzamelt de paden).
        uit.push(waarde("MultiSelectModel", "Player".to_string()));
        uit.push(Registerwaarde {
            sleutel: format!("{basis}\\command"),
            naam: "",
            waarde: format!("\"{exe}\" --actie {ACTIE} \"%1\""),
        });
    }
    uit
}

/// Het werkwoord schrijven (een eerdere versie ervan gaat eerst weg).
#[cfg(windows)]
pub fn registreer_windows(exe: &str, sleutel: &str, label: &str) -> std::io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    use winreg::RegKey;

    verwijder_windows(sleutel)?;
    let klassen = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Classes", KEY_ALL_ACCESS)?;
    for w in windows_waarden(exe, sleutel, label) {
        let (k, _) = klassen.create_subkey(&w.sleutel)?;
        k.set_value(w.naam, &w.waarde)?;
    }
    Ok(())
}

/// Het werkwoord weghalen (bestaat het niet, dan is dat ook goed).
#[cfg(windows)]
pub fn verwijder_windows(sleutel: &str) -> std::io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    use winreg::RegKey;

    let klassen = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Classes", KEY_ALL_ACCESS)?;
    for soort in SOORTEN {
        match klassen.delete_subkey_all(format!("{soort}\\shell\\{sleutel}")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Bij een selectie van meerdere items start de Verkenner de app tegelijk een keer per item. De single-instance-plugin
/// stuurt een tweede start door naar het venster van de eerste, maar bestaat dat venster nog niet (de eerste start is
/// net begonnen), dan zou de tweede een eigen app worden. Daarom: is de app al aan het starten (de mutex van de plugin
/// bestaat), wacht dan even tot zijn venster er is; daarna stuurt de plugin de paden gewoon door.
#[cfg(windows)]
pub fn wacht_op_eerste_start(identifier: &str) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

    let mutex = HSTRING::from(format!("{identifier}-sim"));
    let (klasse, titel) = (HSTRING::from(format!("{identifier}-sic")), HSTRING::from(format!("{identifier}-siw")));
    let Ok(handle) = (unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &mutex) }) else { return };
    let _ = unsafe { CloseHandle(handle) };
    for _ in 0..100 {
        if unsafe { FindWindowW(&klasse, &titel) }.is_ok_and(|h| !h.is_invalid()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Een bestand dat het menu-item vormt, met een pad ten opzichte van zijn basismap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menubestand {
    pub pad: &'static str,
    pub inhoud: Vec<u8>,
    pub uitvoerbaar: bool,
}

/// Het Nautilus-script (onder "Scripts" in het rechtermuismenu).
pub const NAUTILUS_SCRIPT: &str = "nautilus/scripts/Versturen met OpenAEC";
/// De nautilus-python-extensie (het item direct in het menu), als nautilus-python er is.
pub const NAUTILUS_PYTHON: &str = "nautilus-python/extensions/openaec_versturen.py";
/// Het service-menu van Dolphin (KDE).
pub const DOLPHIN: &str = "kio/servicemenus/openaec-versturen.desktop";
/// De actie van Nemo (Cinnamon).
pub const NEMO: &str = "nemo/actions/openaec-versturen.nemo_action";
/// Het pictogram voor Dolphin en Nemo (zo werkt het ook met een AppImage, die geen pictogram in het thema zet).
pub const PICTOGRAM: &str = "openaec-cloud/versturen.png";
/// Alles wat de app op Linux in `~/.local/share` zet (ook `linux/postrm` ruimt precies deze op).
pub const LINUX_PADEN: [&str; 5] = [NAUTILUS_SCRIPT, NAUTILUS_PYTHON, DOLPHIN, NEMO, PICTOGRAM];

/// Een argument voor een shellscript: tussen enkele aanhalingstekens.
pub fn sh_woord(tekst: &str) -> String {
    format!("'{}'", tekst.replace('\'', r"'\''"))
}

/// Het programma in de `Exec`-regel van een .desktop-bestand (of een actie van Nemo): tussen dubbele
/// aanhalingstekens met de tekens die daar speciaal zijn geëscapet, en daarna nog eens de backslashes van de
/// sleutelwaarde zelf (zie de Desktop Entry Specification).
pub fn desktop_exec(exe: &str) -> String {
    let mut binnen = String::new();
    for c in exe.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                binnen.push('\\');
                binnen.push(c);
            }
            '%' => binnen.push_str("%%"),
            _ => binnen.push(c),
        }
    }
    format!("\"{binnen}\"").replace('\\', "\\\\")
}

/// De menubestanden voor Linux. `exe` is het programma (de AppImage zelf, of /usr/bin/openaec-cloud), `pictogram` het
/// volledige pad van het pictogram en `nautilus_python` of de extensie in plaats van het script komt.
pub fn linux_bestanden(exe: &str, pictogram: &str, nautilus_python: bool) -> Vec<Menubestand> {
    let mut uit = Vec::new();
    if nautilus_python {
        let exe_py = serde_json::to_string(exe).unwrap_or_default();
        uit.push(Menubestand {
            pad: NAUTILUS_PYTHON,
            uitvoerbaar: false,
            inhoud: format!(
                r#"# {LABEL}: geplaatst door OpenAEC Desktop App, die het weer opruimt als je het uitzet of de app verwijdert.
import subprocess

import gi

try:
    gi.require_version("Nautilus", "4.0")
except ValueError:
    gi.require_version("Nautilus", "3.0")
from gi.repository import GObject, Nautilus

PROGRAMMA = {exe_py}


class OpenAECVersturen(GObject.GObject, Nautilus.MenuProvider):
    def _versturen(self, _item, bestanden):
        paden = [b.get_location().get_path() for b in bestanden]
        paden = [p for p in paden if p]
        if paden:
            subprocess.Popen([PROGRAMMA, "--actie", "{ACTIE}", *paden], start_new_session=True)

    # Nautilus 4 geeft (bestanden), Nautilus 3 (venster, bestanden).
    def get_file_items(self, *args):
        bestanden = args[-1]
        if not bestanden or any(b.get_location().get_path() is None for b in bestanden):
            return []
        item = Nautilus.MenuItem(name="OpenAECVersturen::versturen", label="{LABEL}")
        item.connect("activate", self._versturen, bestanden)
        return [item]
"#
            )
            .into_bytes(),
        });
    } else {
        uit.push(Menubestand {
            pad: NAUTILUS_SCRIPT,
            uitvoerbaar: true,
            inhoud: format!(
                "#!/bin/sh\n# {LABEL}: geplaatst door OpenAEC Desktop App, die het weer opruimt als je het uitzet of de app verwijdert.\n\
                 # Nautilus geeft de gekozen paden, een per regel.\nIFS='\n'\nset -f\nexec {} --actie {ACTIE} $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\n",
                sh_woord(exe)
            )
            .into_bytes(),
        });
    }
    let exec = desktop_exec(exe);
    uit.push(Menubestand {
        pad: DOLPHIN,
        // KDE 6 neemt alleen een uitvoerbaar service-menu.
        uitvoerbaar: true,
        inhoud: format!(
            "[Desktop Entry]\nType=Service\nMimeType=all/all;\nX-KDE-ServiceTypes=KonqPopupMenu/Plugin\nActions=versturen\n\
             X-KDE-Priority=TopLevel\n\n[Desktop Action versturen]\nName={LABEL}\nIcon={pictogram}\nExec={exec} --actie {ACTIE} %F\n"
        )
        .into_bytes(),
    });
    uit.push(Menubestand {
        pad: NEMO,
        uitvoerbaar: false,
        inhoud: format!(
            "[Nemo Action]\nName={LABEL}\nComment=Bestanden en mappen versturen met een downloadlink\nExec={exec} --actie {ACTIE} %F\n\
             Icon-Name=openaec-cloud\nSelection=notnone\nExtensions=any;\nQuote=double\n"
        )
        .into_bytes(),
    });
    uit
}

/// XML-tekst (voor een plist).
fn xml(tekst: &str) -> String {
    tekst.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// De map van de snelle actie op macOS, ten opzichte van `~/Library/Services`.
pub const MACOS_WORKFLOW: &str = "Versturen met OpenAEC.workflow";

/// De bestanden van de snelle actie voor de Finder (macOS), ten opzichte van `~/Library/Services`: een workflow met
/// één stap "Voer shellscript uit" die de app met de gekozen paden start.
pub fn macos_bestanden(exe: &str) -> Vec<Menubestand> {
    let opdracht = xml(&format!("{} --actie {ACTIE} \"$@\" >/dev/null 2>&1 &", sh_woord(exe)));
    let info = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSBackgroundColorName</key>
			<string>background</string>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{LABEL}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.item</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>523</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{opdracht}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>6B4E2C1A-3F0D-4C8E-9A57-1D2E3F405161</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
				</array>
				<key>OutputUUID</key>
				<string>7C5F3D2B-401E-4D9F-AB68-2E3F40516272</string>
				<key>UUID</key>
				<string>8D604E3C-512F-4EA0-BC79-3F4051627383</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict/>
				<key>isViewVisible</key>
				<integer>1</integer>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleID</key>
		<string>com.apple.finder</string>
		<key>applicationBundleIDsByPath</key>
		<dict/>
		<key>applicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>applicationPaths</key>
		<array>
			<string>/System/Library/CoreServices/Finder.app</string>
		</array>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<integer>0</integer>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#
    );
    vec![
        Menubestand { pad: "Versturen met OpenAEC.workflow/Contents/Info.plist", inhoud: info.into_bytes(), uitvoerbaar: false },
        Menubestand { pad: "Versturen met OpenAEC.workflow/Contents/document.wflow", inhoud: document.into_bytes(), uitvoerbaar: false },
    ]
}

/// De bestanden in `basis` zetten; alleen schrijven wat anders is. Geeft of er iets veranderde.
pub fn installeer_in(basis: &Path, bestanden: &[Menubestand]) -> std::io::Result<bool> {
    let mut veranderd = false;
    for b in bestanden {
        let pad = basis.join(b.pad);
        if std::fs::read(&pad).ok().as_deref() != Some(&b.inhoud[..]) {
            std::fs::create_dir_all(pad.parent().unwrap_or(basis))?;
            std::fs::write(&pad, &b.inhoud)?;
            veranderd = true;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modus = if b.uitvoerbaar { 0o755 } else { 0o644 };
            if std::fs::metadata(&pad)?.permissions().mode() & 0o777 != modus {
                std::fs::set_permissions(&pad, std::fs::Permissions::from_mode(modus))?;
            }
        }
    }
    Ok(veranderd)
}

/// Deze paden uit `basis` weghalen (wat er niet is, is ook goed).
pub fn verwijder_uit(basis: &Path, paden: &[&str]) -> std::io::Result<()> {
    for pad in paden {
        match std::fs::remove_file(basis.join(pad)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Is nautilus-python geïnstalleerd (dan kan het item direct in het menu van Nautilus)?
#[cfg(target_os = "linux")]
fn heeft_nautilus_python() -> bool {
    let mappen = ["/usr/lib", "/usr/lib64", "/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu"];
    mappen.iter().any(|m| {
        ["extensions-4", "extensions-3.0"].iter().any(|e| Path::new(m).join("nautilus").join(e).join("libnautilus-python.so").exists())
    })
}

/// Het programma dat het menu start: bij een AppImage de AppImage zelf (het programma erin staat in een tijdelijke map).
#[cfg(target_os = "linux")]
fn linux_programma() -> std::io::Result<String> {
    match std::env::var("APPIMAGE") {
        Ok(a) if !a.is_empty() => Ok(a),
        _ => Ok(std::env::current_exe()?.to_string_lossy().to_string()),
    }
}

/// `~/.local/share` (Linux) of `~/Library/Services` (macOS).
#[cfg_attr(windows, allow(dead_code))]
fn basismap() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library").join("Services"))
    } else {
        dirs::data_dir()
    }
}

/// Het item aan- of uitzetten voor dit platform, voor de gebruiker die de app draait.
pub fn werk_bij(aan: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if aan {
            registreer_windows(&std::env::current_exe()?.to_string_lossy(), SLEUTEL, LABEL)
        } else {
            verwijder_windows(SLEUTEL)
        }
    }
    #[cfg(target_os = "linux")]
    {
        let basis = basismap().ok_or_else(|| std::io::Error::other("geen gegevensmap"))?;
        if !aan {
            return verwijder_uit(&basis, &LINUX_PADEN);
        }
        let pictogram = Menubestand { pad: PICTOGRAM, inhoud: include_bytes!("../icons/128x128.png").to_vec(), uitvoerbaar: false };
        installeer_in(&basis, std::slice::from_ref(&pictogram))?;
        let python = heeft_nautilus_python();
        let bestanden = linux_bestanden(&linux_programma()?, &basis.join(PICTOGRAM).to_string_lossy(), python);
        // Het script of de extensie, niet allebei (anders staat het item twee keer in Nautilus).
        verwijder_uit(&basis, &[if python { NAUTILUS_SCRIPT } else { NAUTILUS_PYTHON }])?;
        installeer_in(&basis, &bestanden).map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        let basis = basismap().ok_or_else(|| std::io::Error::other("geen thuismap"))?;
        if !aan {
            return match std::fs::remove_dir_all(basis.join(MACOS_WORKFLOW)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if installeer_in(&basis, &macos_bestanden(&std::env::current_exe()?.to_string_lossy()))? {
            // De Finder kent een nieuwe of gewijzigde snelle actie meteen, niet pas na opnieuw inloggen.
            let _ = std::process::Command::new("/System/Library/CoreServices/pbs").arg("-update").status();
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (aan, basismap());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_werkwoord_op_alle_bestanden_en_mappen() {
        let exe = r"C:\Users\Jan\AppData\Local\OpenAEC Desktop App\openaec-cloud.exe";
        let w = windows_waarden(exe, SLEUTEL, LABEL);
        let zoek = |sleutel: &str, naam: &str| w.iter().find(|r| r.sleutel == sleutel && r.naam == naam).map(|r| r.waarde.as_str());
        for soort in ["*", "Directory"] {
            let basis = format!("{soort}\\shell\\OpenAECVersturen");
            assert_eq!(zoek(&basis, "MUIVerb"), Some("Versturen met OpenAEC"));
            assert_eq!(zoek(&basis, "Icon"), Some(format!("\"{exe}\",0").as_str()));
            assert_eq!(zoek(&basis, "MultiSelectModel"), Some("Player"));
            assert_eq!(zoek(&format!("{basis}\\command"), ""), Some(format!("\"{exe}\" --actie versturen \"%1\"").as_str()));
        }
        assert_eq!(w.len(), 8);
    }

    #[test]
    fn aanhalingstekens_voor_shell_en_desktop() {
        assert_eq!(sh_woord("/opt/Open AEC/app"), "'/opt/Open AEC/app'");
        assert_eq!(sh_woord("/home/jan's/app"), r"'/home/jan'\''s/app'");
        assert_eq!(desktop_exec("/usr/bin/openaec-cloud"), "\"/usr/bin/openaec-cloud\"");
        assert_eq!(desktop_exec("/home/jan/Apps/OpenAEC 100%.AppImage"), "\"/home/jan/Apps/OpenAEC 100%%.AppImage\"");
        // `$` en `"` krijgen een backslash, en die backslash wordt in de sleutelwaarde nog eens geëscapet.
        assert_eq!(desktop_exec("/a/$b\"c"), r#""/a/\\$b\\"c""#);
    }

    #[test]
    fn linux_menus_roepen_de_app_aan_met_alle_paden() {
        let exe = "/home/jan/Apps/OpenAEC Desktop App.AppImage";
        let b = linux_bestanden(exe, "/home/jan/.local/share/openaec-cloud/versturen.png", false);
        let tekst = |pad: &str| String::from_utf8(b.iter().find(|m| m.pad == pad).unwrap().inhoud.clone()).unwrap();
        assert_eq!(b.iter().map(|m| m.pad).collect::<Vec<_>>(), [NAUTILUS_SCRIPT, DOLPHIN, NEMO]);
        let script = tekst(NAUTILUS_SCRIPT);
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("exec '/home/jan/Apps/OpenAEC Desktop App.AppImage' --actie versturen $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"));
        assert!(b.iter().find(|m| m.pad == NAUTILUS_SCRIPT).unwrap().uitvoerbaar);
        let dolphin = tekst(DOLPHIN);
        assert!(dolphin.contains("MimeType=all/all;"));
        assert!(dolphin.contains("Name=Versturen met OpenAEC\n"));
        assert!(dolphin.contains("Exec=\"/home/jan/Apps/OpenAEC Desktop App.AppImage\" --actie versturen %F\n"));
        assert!(dolphin.contains("Icon=/home/jan/.local/share/openaec-cloud/versturen.png\n"));
        let nemo = tekst(NEMO);
        assert!(nemo.starts_with("[Nemo Action]\n"));
        assert!(nemo.contains("Exec=\"/home/jan/Apps/OpenAEC Desktop App.AppImage\" --actie versturen %F\n"));
        assert!(nemo.contains("Selection=notnone\n"));

        // Met nautilus-python de extensie in plaats van het script.
        let b = linux_bestanden("/usr/bin/openaec-cloud", "/x.png", true);
        assert_eq!(b[0].pad, NAUTILUS_PYTHON);
        let py = String::from_utf8(b[0].inhoud.clone()).unwrap();
        assert!(py.contains("PROGRAMMA = \"/usr/bin/openaec-cloud\""));
        assert!(py.contains("[PROGRAMMA, \"--actie\", \"versturen\", *paden]"));
        assert!(py.contains("Nautilus.MenuProvider"));
    }

    #[test]
    fn postrm_ruimt_precies_de_linux_bestanden_op() {
        let postrm = include_str!("../linux/postrm");
        for pad in LINUX_PADEN {
            assert!(postrm.contains(pad), "{pad} ontbreekt in linux/postrm");
        }
    }

    #[test]
    fn macos_snelle_actie() {
        let b = macos_bestanden("/Applications/OpenAEC Desktop App.app/Contents/MacOS/openaec-cloud");
        assert_eq!(b.len(), 2);
        assert!(b.iter().all(|m| m.pad.starts_with(MACOS_WORKFLOW)));
        let info = String::from_utf8(b[0].inhoud.clone()).unwrap();
        assert!(info.contains("<string>Versturen met OpenAEC</string>"));
        assert!(info.contains("<string>public.item</string>"));
        let doc = String::from_utf8(b[1].inhoud.clone()).unwrap();
        assert!(doc.contains(
            "<string>'/Applications/OpenAEC Desktop App.app/Contents/MacOS/openaec-cloud' --actie versturen \"$@\" &gt;/dev/null 2&gt;&amp;1 &amp;</string>"
        ));
        assert!(doc.contains("<string>com.apple.Automator.servicesMenu</string>"));
    }

    #[test]
    fn installeren_schrijft_alleen_wat_anders_is_en_verwijderen_ruimt_op() {
        let basis = tempfile_map();
        let bestanden = linux_bestanden("/usr/bin/openaec-cloud", "/x.png", false);
        assert!(installeer_in(&basis, &bestanden).unwrap());
        assert!(!installeer_in(&basis, &bestanden).unwrap());
        assert!(basis.join(DOLPHIN).is_file());
        verwijder_uit(&basis, &LINUX_PADEN).unwrap();
        assert!(!basis.join(DOLPHIN).exists() && !basis.join(NAUTILUS_SCRIPT).exists());
        let _ = std::fs::remove_dir_all(&basis);
    }

    fn tempfile_map() -> PathBuf {
        let m = std::env::temp_dir().join(format!("openaec-rechtermuis-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&m);
        m
    }
}
