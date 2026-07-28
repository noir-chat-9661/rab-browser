import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { render } from "solid-js/web";
import "./styles.css";

type Tab = {
  id: number;
  url: string;
  title: string;
  faviconUrl: string | null;
  canGoBack: boolean;
  canGoForward: boolean;
};

type BrowserState = {
  type: "state";
  tabs: Tab[];
  currentTabId: number | null;
};

type ChromeApi = {
  receive: (state: BrowserState) => void;
  openLocation: () => void;
};

declare global {
  interface Window {
    ipc?: { postMessage: (message: string) => void };
    rabChrome?: ChromeApi;
  }
}

const emptyState: BrowserState = {
  type: "state",
  tabs: [],
  currentTabId: null,
};

function send(message: Record<string, unknown>) {
  window.ipc?.postMessage(JSON.stringify(message));
}

const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.platform);

function hasPrimaryModifier(event: KeyboardEvent | MouseEvent) {
  return isMac ? event.metaKey : event.ctrlKey;
}

function hasSecondaryPrimaryModifier(event: KeyboardEvent | MouseEvent) {
  return isMac ? event.ctrlKey : event.metaKey;
}

function shortcutLabel(key: string) {
  return `${isMac ? "⌘" : "Ctrl+"}${key}`;
}

// Must match crates/browser-app/src/main.rs's NEW_TAB_URL exactly (not a
// prefix match) so an arbitrary data:text/html page a user navigates to
// isn't mistaken for the new-tab placeholder.
const NEW_TAB_URL =
  "data:text/html;charset=utf-8,%3C!doctype%20html%3E%3Chtml%20lang=%22ja%22%3E%3Chead%3E%3Cmeta%20charset=%22utf-8%22%3E%3Ctitle%3E%E6%96%B0%E3%81%97%E3%81%84%E3%82%BF%E3%83%96%3C/title%3E%3Cstyle%3Ehtml%2Cbody%7Bheight%3A100%25%7Dbody%7Bmargin%3A0%3Bdisplay%3Agrid%3Bplace-items%3Acenter%3Bbackground%3A%23171816%3Bcolor%3A%23a2a59d%3Bfont%3A14px%20system-ui%2Csans-serif%7D%3C/style%3E%3C/head%3E%3Cbody%3E%E6%96%B0%E3%81%97%E3%81%84%E3%82%BF%E3%83%96%3C/body%3E%3C/html%3E";

function isNewTabUrl(url: string) {
  return url === "about:blank" || url === NEW_TAB_URL;
}

function displayTitle(tab: Tab) {
  if (tab.title.trim()) return tab.title;
  if (isNewTabUrl(tab.url)) return "New Tab";
  try {
    return new URL(tab.url).hostname.replace(/^www\./, "") || tab.url;
  } catch {
    return tab.url || "New Tab";
  }
}

function App() {
  const [state, setState] = createSignal(emptyState);
  const [locationOpen, setLocationOpen] = createSignal(false);
  const [locationValue, setLocationValue] = createSignal("");
  let locationInput: HTMLInputElement | undefined;

  const currentTab = createMemo(() =>
    state().tabs.find((tab) => tab.id === state().currentTabId),
  );
  const searchMode = createMemo(() => locationValue().startsWith("?"));
  const displayedLocationValue = createMemo(() =>
    searchMode() ? locationValue().slice(1) : locationValue(),
  );

  const closeLocation = () => {
    if (!locationOpen()) return;
    setLocationOpen(false);
    send({ type: "palette_closed" });
  };

  const openLocation = () => {
    setLocationValue(
      isNewTabUrl(currentTab()?.url ?? "") ? "" : currentTab()?.url ?? "",
    );
    if (!locationOpen()) {
      setLocationOpen(true);
      send({ type: "palette_opened" });
    }
    queueMicrotask(() => {
      locationInput?.focus();
      locationInput?.select();
    });
  };

  const navigate = () => {
    const url = locationValue().trim();
    if (!url) return;
    send({ type: "navigate", url });
    closeLocation();
  };

  onMount(() => {
    window.rabChrome = {
      receive: setState,
      openLocation,
    };
    send({ type: "chrome_ready" });

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && locationOpen()) {
        event.preventDefault();
        closeLocation();
        return;
      }

      if (
        !hasPrimaryModifier(event) ||
        event.altKey ||
        hasSecondaryPrimaryModifier(event) ||
        event.shiftKey ||
        event.repeat
      ) return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        event.preventDefault();
        send({ type: "new_tab" });
      } else if (key === "l") {
        event.preventDefault();
        openLocation();
      } else if (key === "r") {
        event.preventDefault();
        if (!isNewTabUrl(currentTab()?.url ?? "")) {
          send({ type: "reload" });
        }
      } else if (key === "s") {
        event.preventDefault();
        send({ type: "toggle_sidebar" });
      } else if (key === "w") {
        event.preventDefault();
        send({ type: "close_current_tab" });
      }
    });
  });

  return (
    <main class="chrome-shell">
      <div class="sidebar-panel">
        <header class="toolbar">
          <div class="history-controls">
            <button
              class="icon-button"
              type="button"
              aria-label="Go back"
              disabled={!currentTab()?.canGoBack}
              onClick={() => send({ type: "go_back" })}
            >
              <ArrowLeft />
            </button>
            <button
              class="icon-button"
              type="button"
              aria-label="Go forward"
              disabled={!currentTab()?.canGoForward}
              onClick={() => send({ type: "go_forward" })}
            >
              <ArrowRight />
            </button>
            <button
              class="icon-button"
              type="button"
              aria-label="Reload"
              disabled={isNewTabUrl(currentTab()?.url ?? "")}
              onClick={() => {
                if (!isNewTabUrl(currentTab()?.url ?? "")) {
                  send({ type: "reload" });
                }
              }}
            >
              <Reload />
            </button>
          </div>
        </header>

        <button class="location-trigger" type="button" onClick={openLocation}>
          <Search />
          <span>
            {isNewTabUrl(currentTab()?.url ?? "")
              ? "Go to a URL"
              : currentTab()?.url}
          </span>
          <kbd>{shortcutLabel("L")}</kbd>
        </button>

        <section class="tabs-section" aria-label="Open tabs">
          <div class="section-label">
            <span>Tabs</span>
            <span>{state().tabs.length.toString().padStart(2, "0")}</span>
          </div>
          <div class="tab-list">
            <For each={state().tabs}>
              {(tab) => (
                <button
                  class="tab"
                  classList={{ active: tab.id === state().currentTabId }}
                  type="button"
                  onClick={() => send({ type: "select_tab", id: tab.id })}
                >
                  <span
                    class="tab-favicon"
                    classList={{ "has-image": Boolean(tab.faviconUrl) }}
                  >
                    <Show
                      when={tab.faviconUrl}
                      fallback={displayTitle(tab).slice(0, 1).toUpperCase()}
                    >
                      {(faviconUrl) => (
                        <img src={faviconUrl()} alt="" aria-hidden="true" />
                      )}
                    </Show>
                  </span>
                  <span class="tab-copy">
                    <span class="tab-title">{displayTitle(tab)}</span>
                    <span class="tab-host">
                      {isNewTabUrl(tab.url) ? "Ready to browse" : tab.url}
                    </span>
                  </span>
                  <span
                    class="tab-close"
                    role="button"
                    tabindex="0"
                    aria-label={`Close ${displayTitle(tab)}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      send({ type: "close_tab", id: tab.id });
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        event.stopPropagation();
                        send({ type: "close_tab", id: tab.id });
                      }
                    }}
                  >
                    <Close />
                  </span>
                </button>
              )}
            </For>
          </div>
        </section>

        <button
          class="new-tab"
          type="button"
          onClick={() => send({ type: "new_tab" })}
        >
          <Plus />
          <span>New tab</span>
          <kbd>{shortcutLabel("T")}</kbd>
        </button>
      </div>

      <Show when={locationOpen()}>
        <div class="palette-backdrop" onClick={closeLocation}>
          <form
            class="command-palette"
            role="dialog"
            aria-modal="true"
            aria-labelledby="location-label"
            onSubmit={(event) => {
              event.preventDefault();
              navigate();
            }}
            onClick={(event) => event.stopPropagation()}
          >
            <label id="location-label" for="location">Navigate</label>
            <div
              class="command-input-wrap"
              classList={{ "search-mode": searchMode() }}
            >
              <Search />
              <Show when={searchMode()}>
                <span class="search-provider">Google</span>
              </Show>
              <input
                ref={locationInput}
                id="location"
                value={displayedLocationValue()}
                onInput={(event) =>
                  setLocationValue(
                    searchMode()
                      ? `?${event.currentTarget.value}`
                      : event.currentTarget.value,
                  )
                }
                onKeyDown={(event) => {
                  if (
                    event.key === "Backspace" &&
                    searchMode() &&
                    displayedLocationValue() === ""
                  ) {
                    event.preventDefault();
                    setLocationValue("");
                  }
                }}
                autocomplete="off"
                autocapitalize="off"
                spellcheck={false}
                placeholder={
                  searchMode() ? "ウェブを検索します" : "URL or domain"
                }
              />
            </div>
            <div class="command-hint">
              <span>Open in current tab</span>
              <kbd>↵</kbd>
            </div>
          </form>
        </div>
      </Show>
    </main>
  );
}

function ArrowLeft() {
  return <svg viewBox="0 0 20 20"><path d="m12.5 4.5-5.5 5.5 5.5 5.5M7 10h7" /></svg>;
}

function ArrowRight() {
  return <svg viewBox="0 0 20 20"><path d="m7.5 4.5 5.5 5.5-5.5 5.5M13 10H6" /></svg>;
}

function Reload() {
  return <svg viewBox="0 0 20 20"><path d="M15.2 6.8A6 6 0 1 0 16 11M15.2 6.8V3.5m0 3.3h-3.3" /></svg>;
}

function Search() {
  return <svg viewBox="0 0 20 20"><circle cx="8.5" cy="8.5" r="4.5" /><path d="m12 12 4 4" /></svg>;
}

function Plus() {
  return <svg viewBox="0 0 20 20"><path d="M10 4v12M4 10h12" /></svg>;
}

function Close() {
  return <svg viewBox="0 0 20 20"><path d="m6 6 8 8m0-8-8 8" /></svg>;
}

render(() => <App />, document.getElementById("root")!);
