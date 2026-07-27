import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { render } from "solid-js/web";
import "./styles.css";

type Tab = {
  id: number;
  url: string;
  title: string;
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

function isNewTabUrl(url: string) {
  return (
    url === "about:blank" ||
    url.startsWith("data:text/html;charset=utf-8,")
  );
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
        !event.metaKey ||
        event.altKey ||
        event.ctrlKey ||
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
      }
    });
  });

  return (
    <main class="chrome-shell">
      <div class="sidebar-panel">
        <header class="toolbar">
          <div class="brand" aria-label="rab browser">
            <span class="brand-mark">r</span>
            <span class="brand-name">rab</span>
          </div>
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
              onClick={() => send({ type: "reload" })}
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
          <kbd>⌘L</kbd>
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
                  <span class="tab-favicon">
                    {displayTitle(tab).slice(0, 1).toUpperCase()}
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
          <kbd>⌘T</kbd>
        </button>

        <footer>
          <span class="status-dot" />
          <span>Local session</span>
        </footer>
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
            <div class="command-input-wrap">
              <Search />
              <input
                ref={locationInput}
                id="location"
                value={locationValue()}
                onInput={(event) => setLocationValue(event.currentTarget.value)}
                autocomplete="off"
                autocapitalize="off"
                spellcheck={false}
                placeholder="URL or domain"
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
