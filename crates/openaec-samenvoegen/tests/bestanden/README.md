# Testbestanden

Echte IFC-bestanden voor de tests. De tests passen ze zelf aan (een naam, een eigenschap, een taakdatum, een object
erbij of eraf) en nummeren ze opnieuw. Deze bestanden zelf blijven ongewijzigd.

## Planningen van Open Planner Studio (IFC4X3_ADD2)

Uit ons eigen project [OpenAEC-Foundation/open-planner-studio](https://github.com/OpenAEC-Foundation/open-planner-studio),
map `examples/`, commit `42c63685b73e7febb2867274a446835b897fc825` (26 oktober 2026). Licentie: LGPL-3.0, zoals de rest
van dat project.

- `showcase-groot-terrein-onderaannemer.ifc`
- `13-supermarkt-albert-heijn.ifc`
- `05-brugvervanging-n279.ifc`

### Gewijzigde planningen (`planning/`)

Gemaakt met `scripts/ops/maak_planningen.sh` en de code van OPS op commit `604dd2940` (3 november 2026): de
supermarktplanning hierboven opnieuw opgeslagen (`basis.ifc`), en per bestand één wijziging zoals een planner hem in
OPS maakt, doorgerekend en opgeslagen. De namen zeggen wat er gewijzigd is (bijv. `duur-ons.ifc`, `relatie-hun.ifc`).
`relatie-lag150-ons`, `relatie-lag150.4-hun`, `relatie-lag240-ons`, `relatie-halvedag-*` en `relatie-p6-hun` hebben
dezelfde nieuwe relatie met een lag of de P6-grens; een deel daarvan staat in `OPS_Sequences`. Licentie: LGPL-3.0,
zoals OPS.

## Bouwmodellen met geometrie (IFC2X3 en IFC4)

Uit [buildingSMART/Sample-Test-Files](https://github.com/buildingSMART/Sample-Test-Files), commit
`80d976a9b193a26a8e928c3e79bff67af1de68a8`. Licentie: Creative Commons Attribution 4.0 International (CC BY 4.0),
© buildingSMART International Ltd. Ongewijzigd overgenomen; alleen de bestandsnaam is anders.

| Hier | Daar |
|---|---|
| `ifc2x3-building-architecture.ifc` | `IFC 2.3.0.1 (IFC 2x3 TC1)/Simple-Scene/Building-Architecture.ifc` |
| `ifc4-wall-with-opening-and-window.ifc` | `IFC 4.0.2.1 (IFC 4 ADD2 TC1)/ISO Spec - ReferenceView_V1.2/wall-with-opening-and-window.ifc` |
| `ifc4-tessellation-with-individual-colors.ifc` | `IFC 4.0.2.1 (IFC 4 ADD2 TC1)/ISO Spec - ReferenceView_V1.2/tessellation-with-individual-colors.ifc` |
