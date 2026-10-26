// Markdown van de assistent als gewone elementen met tekst (zie lib/markdown.ts): geen innerHTML, dus geen
// HTML-injectie. Links openen in de browser (alleen http, https en mailto); een codeblok heeft een knop Kopiëren.

import { createMemo, createSignal, For, Match, Show, Switch, type JSX } from "solid-js"
import { Dynamic } from "solid-js/web"
import { open } from "@tauri-apps/plugin-shell"
import { api } from "./lib/api"
import { parseMarkdown, type Blok, type Inline } from "./lib/markdown"

function InlineDelen(props: { delen: Inline[] }): JSX.Element {
  return (
    <For each={props.delen}>
      {(d) => (
        <Switch>
          <Match when={d.soort === "tekst" && d}>{(x) => x().tekst}</Match>
          <Match when={d.soort === "code" && d}>{(x) => <code>{x().tekst}</code>}</Match>
          <Match when={d.soort === "breuk"}>
            <br />
          </Match>
          <Match when={d.soort === "vet" && d}>
            {(x) => (
              <strong>
                <InlineDelen delen={x().inhoud} />
              </strong>
            )}
          </Match>
          <Match when={d.soort === "cursief" && d}>
            {(x) => (
              <em>
                <InlineDelen delen={x().inhoud} />
              </em>
            )}
          </Match>
          <Match when={d.soort === "doorgehaald" && d}>
            {(x) => (
              <s>
                <InlineDelen delen={x().inhoud} />
              </s>
            )}
          </Match>
          <Match when={d.soort === "link" && d}>
            {(x) => (
              <a
                href={x().href}
                title={x().href}
                onClick={(e) => {
                  e.preventDefault()
                  open(x().href).catch(() => {})
                }}
              >
                <InlineDelen delen={x().inhoud} />
              </a>
            )}
          </Match>
        </Switch>
      )}
    </For>
  )
}

function Codeblok(props: { taal: string; tekst: string }) {
  const [gekopieerd, setGekopieerd] = createSignal(false)
  const kopieer = () =>
    api
      .kopieer(props.tekst)
      .then(() => {
        setGekopieerd(true)
        setTimeout(() => setGekopieerd(false), 1500)
      })
      .catch(() => {})
  return (
    <div class="md-code">
      <div class="md-code-kop">
        <span>{props.taal || "code"}</span>
        <button class="klein" onClick={kopieer}>
          {gekopieerd() ? "Gekopieerd" : "Kopiëren"}
        </button>
      </div>
      <pre>
        <code>{props.tekst}</code>
      </pre>
    </div>
  )
}

const uitlijnen = (u: string | null | undefined) => (u === "midden" ? "center" : u === "rechts" ? "right" : undefined)

function Blokken(props: { blokken: Blok[] }): JSX.Element {
  return (
    <For each={props.blokken}>
      {(b) => (
        <Switch>
          <Match when={b.soort === "alinea" && b}>
            {(x) => (
              <p>
                <InlineDelen delen={x().inhoud} />
              </p>
            )}
          </Match>
          <Match when={b.soort === "kop" && b}>
            {(x) => (
              <Dynamic component={`h${Math.min(6, x().niveau + 2)}`} class="md-kop">
                <InlineDelen delen={x().inhoud} />
              </Dynamic>
            )}
          </Match>
          <Match when={b.soort === "code" && b}>{(x) => <Codeblok taal={x().taal} tekst={x().tekst} />}</Match>
          <Match when={b.soort === "lijst" && b}>
            {(x) => (
              <Dynamic component={x().geordend ? "ol" : "ul"} start={x().geordend && x().start !== 1 ? x().start : undefined}>
                <For each={x().items}>
                  {(item) => (
                    <li>
                      <Blokken blokken={item} />
                    </li>
                  )}
                </For>
              </Dynamic>
            )}
          </Match>
          <Match when={b.soort === "citaat" && b}>
            {(x) => (
              <blockquote>
                <Blokken blokken={x().blokken} />
              </blockquote>
            )}
          </Match>
          <Match when={b.soort === "tabel" && b}>
            {(x) => (
              <div class="md-tabel">
                <table>
                  <thead>
                    <tr>
                      <For each={x().kop}>
                        {(cel, k) => (
                          <th style={{ "text-align": uitlijnen(x().uitlijning[k()]) }}>
                            <InlineDelen delen={cel} />
                          </th>
                        )}
                      </For>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={x().rijen}>
                      {(rij) => (
                        <tr>
                          <For each={rij}>
                            {(cel, k) => (
                              <td style={{ "text-align": uitlijnen(x().uitlijning[k()]) }}>
                                <InlineDelen delen={cel} />
                              </td>
                            )}
                          </For>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
              </div>
            )}
          </Match>
          <Match when={b.soort === "lijn"}>
            <hr />
          </Match>
        </Switch>
      )}
    </For>
  )
}

export default function Markdown(props: { tekst: string }) {
  const blokken = createMemo(() => parseMarkdown(props.tekst))
  return (
    <div class="md">
      <Show when={props.tekst}>
        <Blokken blokken={blokken()} />
      </Show>
    </div>
  )
}
