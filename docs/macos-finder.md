# macOS: statusvinkjes en bestanden op aanvraag in de Finder

Onderzoek, nog niet gebouwd. Op macOS synchroniseert de app, en **Versturen** en **Delen…** staan in het
rechtermuismenu van de Finder (snelle acties). Wat ontbreekt: statusvinkjes, bestanden op aanvraag en een eigen plek in
de navigatiekolom. Op Linux lossen emblemen in de metadata van GVfs de vinkjes op; de Finder heeft zoiets niet. Een
bestand een gekleurd label geven kan wel, maar dat is van de gebruiker en geen status.

## Twee wegen

| | Finder Sync-extensie | File Provider-extensie |
|---|---|---|
| Wat het geeft | vinkjes (badges) op bestanden in mappen die de app aanwijst, en een eigen menu en knop in de Finder | alles: vinkjes en voortgang van de Finder zelf, bestanden op aanvraag, een plek onder **Locaties** |
| Waar de bestanden staan | waar ze nu staan (de gekoppelde map) | in `~/Library/CloudStorage/OpenAEC-…`, door macOS beheerd |
| Sync-motor | blijft zoals hij is | moet achter de extensie komen te hangen (macOS vraagt per bestand om de inhoud) |
| Werk | klein: een extensie van een paar honderd regels Swift | groot: de hele opslag als `NSFileProviderReplicatedExtension` |
| Status bij Apple | bestaat nog, maar Apple stuurt cloud-apps naar File Provider | de bedoelde weg (zo werken iCloud Drive, OneDrive en Dropbox nu) |

Dit zijn de twee wegen die Apple biedt; een derde (pictogrammen van bestanden vervangen) is traag en gaat stuk bij
elke wijziging.

## Wat beide vragen

- **Een extensie in Swift, in de app-bundel** (`Contents/PlugIns/….appex`). Tauri bouwt die niet: er komt een
  Xcode-project (of `swiftc` met een eigen `Info.plist`) naast, en een stap in de bouwstraat die de extensie in de
  bundel zet vóór het ondertekenen.
- **Sandbox en een app-groep.** Een extensie draait altijd in de sandbox. De app en de extensie delen gegevens via een
  app-groep (`group.<team>.com.openaec.cloud`): daar komt de status per pad te staan, zoals `menu.json` op Windows.
  Dat vraagt een entitlement en een provisioning profile voor de extensie; ondertekenen en notariseren doet de
  bouwstraat al.
- **Praten met de app.** De extensie leest de status uit de app-groep en hoort van wijzigingen via een
  Darwin-notificatie (of XPC). De logica voor de status bestaat al en is los van Windows (`itemstatus.rs`,
  `emblemen::gewenst`).

## Voorstel

1. **Eerst de Finder Sync-extensie.** Dezelfde vier statussen als op Linux, in de mappen die al gekoppeld zijn, en
   **Delen…** alleen in een gekoppelde map (de snelle actie staat nu overal). De sync-motor en de plek van de
   bestanden veranderen niet. Te bouwen en te testen op een Mac met Xcode; in de bouwstraat een extra stap op de
   macOS-runner.
2. **Daarna beslissen over File Provider.** Alleen nodig voor bestanden op aanvraag. Het is de macOS-tegenhanger van
   de Cloud Filter-code op Windows (`cloudfilter.rs`, `opaanvraag.rs`, `hoofdmap.rs`) en ongeveer even groot. Wie al
   een gekoppelde map heeft, moet dan verhuizen naar de map van macOS.

## Wat nog onbekend is

- Of een Finder Sync-extensie in een met Tauri gebouwde bundel zonder verdere aanpassing geladen wordt (de bundel-id
  van de extensie moet met die van de app beginnen, en de app moet minstens één keer gestart zijn).
- Of het Developer ID-profiel van de organisatie al een app-groep heeft.
- Hoe lang Apple Finder Sync nog ondersteunt. Tot nu toe is het niet als verouderd gemarkeerd.

Niets hiervan is op een Mac geprobeerd.
