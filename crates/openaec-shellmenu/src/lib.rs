//! OpenAEC in het moderne rechtermuismenu van Windows 11.
//!
//! Windows 11 toont in het korte menu alleen items van een `IExplorerCommand` uit een pakket met identiteit; klassieke
//! registerwerkwoorden staan onder "Meer opties weergeven". Deze crate is die COM-server: een DLL met één hoofditem
//! "OpenAEC" en de subitems "Delen…" (alleen binnen gekoppelde mappen) en "Versturen met OpenAEC" (op alles). Een keuze
//! start de app met `--actie <delen|versturen> <paden…>`, net als de klassieke items.
//!
//! - [`logica`]: wat zichtbaar is, de opdrachtregel, `menu.json` en het manifest; zonder COM, op elk platform.
//! - `server` (alleen Windows, feature `server`): `IExplorerCommand`, `IEnumExplorerCommand`, `IClassFactory` en de
//!   exports `DllGetClassObject`/`DllCanUnloadNow`.

pub mod logica;

#[cfg(all(windows, feature = "server"))]
mod server;
