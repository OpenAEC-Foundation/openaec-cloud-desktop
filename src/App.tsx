import { createSignal, For, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api, type RemoteEntry, type SyncReport } from "./lib/api"

export default function App() {
  const [base, setBase] = createSignal("https://cloud.openaec.lan")
  const [user, setUser] = createSignal("")
  const [pass, setPass] = createSignal("")
  const [connected, setConnected] = createSignal(false)
  const [busy, setBusy] = createSignal(false)
  const [status, setStatus] = createSignal("Niet verbonden")
  const [root, setRoot] = createSignal<RemoteEntry[]>([])
  const [localDir, setLocalDir] = createSignal("")
  const [remoteDir, setRemoteDir] = createSignal("OpenAEC Cloud")
  const [report, setReport] = createSignal<SyncReport | null>(null)
  const [shareOf, setShareOf] = createSignal("")
  const [shareUrl, setShareUrl] = createSignal("")

  const connect = async () => {
    setBusy(true)
    setStatus("Verbinden…")
    try {
      const r = await api.connect(base(), user(), pass())
      setRoot(r)
      setConnected(true)
      setStatus(`Verbonden als ${user()}`)
    } catch (e) {
      setStatus("Verbinden mislukt: " + e)
    }
    setBusy(false)
  }

  const pickLocal = async () => {
    const dir = await open({ directory: true, multiple: false })
    if (typeof dir === "string") setLocalDir(dir)
  }

  const doSync = async () => {
    if (!localDir()) return setStatus("Kies eerst een lokale map")
    setBusy(true)
    setStatus("Synchroniseren…")
    setReport(null)
    try {
      const rep = await api.syncFolder(localDir(), remoteDir())
      setReport(rep)
      setStatus(`Sync klaar — ${rep.uploaded} geüpload, ${rep.downloaded} gedownload`)
    } catch (e) {
      setStatus("Sync mislukt: " + e)
    }
    setBusy(false)
  }

  const makeLink = async () => {
    if (!shareOf()) return
    setBusy(true)
    try {
      setShareUrl(await api.createLink(shareOf()))
      setStatus("Deellink aangemaakt")
    } catch (e) {
      setStatus("Delen mislukt: " + e)
    }
    setBusy(false)
  }

  return (
    <div class="app">
      <header class="titlebar" data-tauri-drag-region>
        <div class="wordmark">
          Open<span>AEC</span> Cloud
        </div>
        <div class="dot" classList={{ on: connected() }} title={connected() ? "verbonden" : "niet verbonden"} />
      </header>

      <main class="body">
        <Show
          when={connected()}
          fallback={
            <section class="card">
              <h2>Verbinden met je OpenAEC Cloud</h2>
              <label>Server<input value={base()} onInput={(e) => setBase(e.currentTarget.value)} /></label>
              <label>Gebruiker<input value={user()} placeholder="maarten@open-aec.com" onInput={(e) => setUser(e.currentTarget.value)} /></label>
              <label>App-wachtwoord<input type="password" value={pass()} onInput={(e) => setPass(e.currentTarget.value)} /></label>
              <p class="hint">Maak een app-wachtwoord in Nextcloud → Instellingen → Beveiliging. (SSO-login volgt in een volgende versie.)</p>
              <button class="primary" disabled={busy()} onClick={connect}>Verbinden</button>
            </section>
          }
        >
          <section class="card">
            <h2>Map synchroniseren</h2>
            <div class="row">
              <button onClick={pickLocal}>Lokale map kiezen…</button>
              <span class="path">{localDir() || "(geen map gekozen)"}</span>
            </div>
            <label>Nextcloud-map<input value={remoteDir()} onInput={(e) => setRemoteDir(e.currentTarget.value)} /></label>
            <button class="primary" disabled={busy() || !localDir()} onClick={doSync}>Nu synchroniseren</button>
            <Show when={report()}>
              {(r) => (
                <div class="report">
                  ↑ {r().uploaded} geüpload · ↓ {r().downloaded} gedownload · = {r().skipped} ongewijzigd
                  <Show when={r().errors.length}>
                    <ul class="errs"><For each={r().errors}>{(err) => <li>{err}</li>}</For></ul>
                  </Show>
                </div>
              )}
            </Show>
          </section>

          <section class="card">
            <h2>Map of bestand delen</h2>
            <div class="row">
              <select onChange={(e) => setShareOf(e.currentTarget.value)}>
                <option value="">— kies uit je cloud —</option>
                <For each={root()}>{(e) => <option value={e.path}>{e.is_dir ? "📁 " : "📄 "}{e.name}</option>}</For>
              </select>
              <button disabled={busy() || !shareOf()} onClick={makeLink}>Deellink maken</button>
            </div>
            <Show when={shareUrl()}>
              <div class="sharelink"><a href={shareUrl()} target="_blank" rel="noreferrer">{shareUrl()}</a></div>
            </Show>
          </section>
        </Show>
      </main>

      <footer class="statusbar">{status()}</footer>
    </div>
  )
}
