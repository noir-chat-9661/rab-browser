import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onMount,
} from "solid-js";
import { render } from "solid-js/web";
import { type Locale, translations } from "./i18n";
import "./styles.css";

type Tab = {
  id: number;
  url: string;
  title: string;
  faviconUrl: string | null;
  canGoBack: boolean;
  canGoForward: boolean;
};

type Bookmark = {
  url: string;
  title: string;
};

type SearchEngine = "google" | "duckduckgo" | "bing";
type Theme = "dark" | "light";

type BrowserState = {
  type: "state";
  tabs: Tab[];
  currentTabId: number | null;
  bookmarks: Bookmark[];
  settings: {
    searchEngine: SearchEngine;
    theme: Theme;
    locale: Locale;
  };
};

type ChromeApi = {
  receive: (state: BrowserState) => void;
  openLocation: () => void;
  openSettings: () => void;
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
  bookmarks: [],
  settings: {
    searchEngine: "google",
    theme: "dark",
    locale: "japanese",
  },
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
  "rab://newtab/";

function isNewTabUrl(url: string) {
  return url === "about:blank" || url === NEW_TAB_URL;
}

function displayTitle(tab: Tab, newTabTitle: string) {
  if (tab.title.trim()) return tab.title;
  if (isNewTabUrl(tab.url)) return newTabTitle;
  try {
    return new URL(tab.url).hostname.replace(/^www\./, "") || tab.url;
  } catch {
    return tab.url || newTabTitle;
  }
}

function displayBookmarkTitle(bookmark: Bookmark) {
  if (bookmark.title.trim()) return bookmark.title;
  try {
    return new URL(bookmark.url).hostname.replace(/^www\./, "") || bookmark.url;
  } catch {
    return bookmark.url;
  }
}

const searchEngines: { value: SearchEngine; label: string; detail: string }[] = [
  { value: "google", label: "Google", detail: "google.com" },
  { value: "duckduckgo", label: "DuckDuckGo", detail: "duckduckgo.com" },
  { value: "bing", label: "Bing", detail: "bing.com" },
];

const themes: Theme[] = [
  "dark",
  "light",
];

const locales: Locale[] = [
  "japanese",
  "english",
];

function App() {
  const [state, setState] = createSignal(emptyState);
  const [locationOpen, setLocationOpen] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [locationValue, setLocationValue] = createSignal("");
  let locationInput: HTMLInputElement | undefined;
  let settingsCloseButton: HTMLButtonElement | undefined;
  const t = createMemo(() => translations[state().settings.locale]);

  const currentTab = createMemo(() =>
    state().tabs.find((tab) => tab.id === state().currentTabId),
  );
  const currentTabBookmarked = createMemo(() => {
    const url = currentTab()?.url;
    return Boolean(url && state().bookmarks.some((bookmark) => bookmark.url === url));
  });
  const searchMode = createMemo(() => locationValue().startsWith("?"));
  const searchEngineLabel = createMemo(
    () =>
      searchEngines.find(
        (engine) => engine.value === state().settings.searchEngine,
      )?.label ?? "Google",
  );
  const displayedLocationValue = createMemo(() =>
    searchMode() ? locationValue().slice(1) : locationValue(),
  );

  createEffect(() => {
    document.documentElement.dataset.theme = state().settings.theme;
    document.documentElement.lang =
      state().settings.locale === "japanese" ? "ja" : "en";
  });

  const closeLocation = () => {
    if (!locationOpen()) return;
    setLocationOpen(false);
    send({ type: "palette_closed" });
  };

  const openLocation = () => {
    setSettingsOpen(false);
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

  const closeSettings = () => {
    if (!settingsOpen()) return;
    setSettingsOpen(false);
    send({ type: "palette_closed" });
  };

  const openSettings = () => {
    setLocationOpen(false);
    if (!settingsOpen()) {
      setSettingsOpen(true);
      send({ type: "palette_opened" });
    }
    queueMicrotask(() => {
      settingsCloseButton?.focus();
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
      openSettings,
    };
    send({ type: "chrome_ready" });

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        if (settingsOpen()) {
          event.preventDefault();
          closeSettings();
          return;
        }
        if (locationOpen()) {
          event.preventDefault();
          closeLocation();
          return;
        }
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
              aria-label={t().goBack}
              disabled={!currentTab()?.canGoBack}
              onClick={() => send({ type: "go_back" })}
            >
              <ArrowLeft />
            </button>
            <button
              class="icon-button"
              type="button"
              aria-label={t().goForward}
              disabled={!currentTab()?.canGoForward}
              onClick={() => send({ type: "go_forward" })}
            >
              <ArrowRight />
            </button>
            <button
              class="icon-button"
              type="button"
              aria-label={t().reload}
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
          <button
            class="icon-button settings-trigger"
            type="button"
            aria-label={t().openSettings}
            onClick={openSettings}
          >
            <Gear />
          </button>
        </header>

        <div class="location-row">
          <button class="location-trigger" type="button" onClick={openLocation}>
            <Search />
            <span>
              {isNewTabUrl(currentTab()?.url ?? "")
                ? t().goToUrl
                : currentTab()?.url}
            </span>
            <kbd>{shortcutLabel("L")}</kbd>
          </button>
          <button
            class="bookmark-toggle"
            classList={{ active: currentTabBookmarked() }}
            type="button"
            aria-label={
              currentTabBookmarked()
                ? t().removeCurrentBookmark
                : t().bookmarkCurrentPage
            }
            aria-pressed={currentTabBookmarked()}
            disabled={isNewTabUrl(currentTab()?.url ?? "")}
            onClick={() => send({ type: "toggle_bookmark" })}
          >
            <Star />
          </button>
        </div>

        <section class="tabs-section" aria-label={t().openTabs}>
          <div class="section-label">
            <span>{t().tabs}</span>
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
                      fallback={displayTitle(tab, t().newTab).slice(0, 1).toUpperCase()}
                    >
                      {(faviconUrl) => (
                        <img src={faviconUrl()} alt="" aria-hidden="true" />
                      )}
                    </Show>
                  </span>
                  <span class="tab-copy">
                    <span class="tab-title">{displayTitle(tab, t().newTab)}</span>
                    <span class="tab-host">
                      {isNewTabUrl(tab.url) ? t().readyToBrowse : tab.url}
                    </span>
                  </span>
                  <span
                    class="tab-close"
                    role="button"
                    tabindex="0"
                    aria-label={t().closeTab(displayTitle(tab, t().newTab))}
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

        <section class="bookmarks-section" aria-label={t().bookmarks}>
          <div class="section-label">
            <span>{t().bookmarks}</span>
            <span>{state().bookmarks.length.toString().padStart(2, "0")}</span>
          </div>
          <Show
            when={state().bookmarks.length > 0}
            fallback={<p class="bookmarks-empty">{t().emptyBookmarks}</p>}
          >
            <div class="bookmark-list">
              <For each={state().bookmarks}>
                {(bookmark) => (
                  <button
                    class="bookmark"
                    type="button"
                    title={bookmark.url}
                    onClick={() =>
                      send({ type: "select_bookmark", url: bookmark.url })
                    }
                  >
                    <span class="bookmark-mark"><Star /></span>
                    <span class="bookmark-copy">
                      <span class="bookmark-title">
                        {displayBookmarkTitle(bookmark)}
                      </span>
                      <span class="bookmark-url">{bookmark.url}</span>
                    </span>
                    <span
                      class="bookmark-remove"
                      role="button"
                      tabindex="0"
                      aria-label={t().removeBookmark(displayBookmarkTitle(bookmark))}
                      onClick={(event) => {
                        event.stopPropagation();
                        send({ type: "remove_bookmark", url: bookmark.url });
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          event.stopPropagation();
                          send({ type: "remove_bookmark", url: bookmark.url });
                        }
                      }}
                    >
                      <Close />
                    </span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </section>

        <button
          class="new-tab"
          type="button"
          onClick={() => send({ type: "new_tab" })}
        >
          <Plus />
          <span>{t().newTab}</span>
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
            <label id="location-label" for="location">{t().navigate}</label>
            <div
              class="command-input-wrap"
              classList={{ "search-mode": searchMode() }}
            >
              <Search />
              <Show when={searchMode()}>
                <span class="search-provider">{searchEngineLabel()}</span>
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
                  searchMode() ? t().searchWeb : t().urlOrDomain
                }
              />
            </div>
            <div class="command-hint">
              <span>{t().openInCurrentTab}</span>
              <kbd>↵</kbd>
            </div>
          </form>
        </div>
      </Show>

      <Show when={settingsOpen()}>
        <div class="palette-backdrop settings-backdrop" onClick={closeSettings}>
          <section
            class="settings-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby="settings-title"
            onClick={(event) => event.stopPropagation()}
          >
            <header class="settings-header">
              <div>
                <span class="settings-eyebrow">{t().browserPreferences}</span>
                <h1 id="settings-title">{t().settings}</h1>
              </div>
              <button
                ref={settingsCloseButton}
                class="icon-button"
                type="button"
                aria-label={t().closeSettings}
                onClick={closeSettings}
              >
                <Close />
              </button>
            </header>
            <div class="settings-content">
              <div class="settings-group">
                <div class="settings-copy">
                  <h2>{t().defaultSearchEngine}</h2>
                  <p>{t().defaultSearchEngineDescription}</p>
                </div>
                <div class="search-engine-options">
                  <For each={searchEngines}>
                    {(engine) => (
                      <label
                        class="search-engine-option"
                        classList={{
                          selected:
                            state().settings.searchEngine === engine.value,
                        }}
                      >
                        <input
                          type="radio"
                          name="search-engine"
                          value={engine.value}
                          checked={
                            state().settings.searchEngine === engine.value
                          }
                          onChange={() =>
                            send({
                              type: "set_search_engine",
                              engine: engine.value,
                            })
                          }
                        />
                        <span class="radio-mark" aria-hidden="true" />
                        <span class="engine-copy">
                          <strong>{engine.label}</strong>
                          <span>{engine.detail}</span>
                        </span>
                      </label>
                    )}
                  </For>
                </div>
              </div>

              <div class="settings-group">
                <div class="settings-copy">
                  <h2>{t().appearance}</h2>
                  <p>{t().appearanceDescription}</p>
                </div>
                <div class="theme-options">
                  <For each={themes}>
                    {(theme) => (
                      <label
                        class="theme-option"
                        classList={{
                          selected: state().settings.theme === theme,
                        }}
                      >
                        <input
                          type="radio"
                          name="theme"
                          value={theme}
                          checked={state().settings.theme === theme}
                          onChange={() =>
                            send({
                              type: "set_theme",
                              theme,
                            })
                          }
                        />
                        <span class="radio-mark" aria-hidden="true" />
                        <span class="engine-copy">
                          <strong>{theme === "dark" ? t().dark : t().light}</strong>
                          <span>{theme === "dark" ? t().darkTheme : t().lightTheme}</span>
                        </span>
                      </label>
                    )}
                  </For>
                </div>
              </div>

              <div class="settings-group">
                <div class="settings-copy">
                  <h2>{t().language}</h2>
                  <p>{t().languageDescription}</p>
                </div>
                <div class="theme-options">
                  <For each={locales}>
                    {(locale) => (
                      <label
                        class="theme-option"
                        classList={{
                          selected: state().settings.locale === locale,
                        }}
                      >
                        <input
                          type="radio"
                          name="locale"
                          value={locale}
                          checked={state().settings.locale === locale}
                          onChange={() =>
                            send({
                              type: "set_locale",
                              locale,
                            })
                          }
                        />
                        <span class="radio-mark" aria-hidden="true" />
                        <span class="engine-copy">
                          <strong>{locale === "japanese" ? t().japanese : t().english}</strong>
                          <span>
                            {locale === "japanese"
                              ? t().japaneseDescription
                              : t().englishDescription}
                          </span>
                        </span>
                      </label>
                    )}
                  </For>
                </div>
              </div>

              <div class="settings-group">
                <div class="settings-copy">
                  <h2>{t().privacy}</h2>
                  <p>{t().privacyDescription}</p>
                </div>
                <div class="privacy-actions">
                  <button
                    class="privacy-action"
                    type="button"
                    onClick={() => send({ type: "clear_history" })}
                  >
                    <span>
                      <strong>{t().clearHistory}</strong>
                      <small>{t().clearHistoryDescription}</small>
                    </span>
                    <Trash />
                  </button>
                  <button
                    class="privacy-action"
                    type="button"
                    onClick={() => send({ type: "clear_cookies" })}
                  >
                    <span>
                      <strong>{t().clearCookies}</strong>
                      <small>{t().clearCookiesDescription}</small>
                    </span>
                    <Cookie />
                  </button>
                </div>
              </div>
            </div>
          </section>
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

function Star() {
  return (
    <svg viewBox="0 0 20 20">
      <path d="m10 3 2.1 4.3 4.7.7-3.4 3.3.8 4.7-4.2-2.2L5.8 16l.8-4.7L3.2 8l4.7-.7L10 3Z" />
    </svg>
  );
}

function Gear() {
  return (
    <svg viewBox="0 0 20 20">
      <circle cx="10" cy="10" r="2.4" />
      <path d="M8.8 3.1h2.4l.5 1.8 1.3.8 1.8-.5 1.2 2.1-1.3 1.3v1.5l1.3 1.3-1.2 2.1-1.8-.5-1.3.8-.5 1.8H8.8l-.5-1.8-1.3-.8-1.8.5L4 11.4l1.3-1.3V8.6L4 7.3l1.2-2.1 1.8.5 1.3-.8.5-1.8Z" />
    </svg>
  );
}

function Trash() {
  return <svg viewBox="0 0 20 20"><path d="M5.5 6.5h9M8 4.5h4M7 6.5l.5 9h5l.5-9M9 9v4m2-4v4" /></svg>;
}

function Cookie() {
  return (
    <svg viewBox="0 0 20 20">
      <path d="M16.5 10a6.5 6.5 0 1 1-6.5-6.5 3 3 0 0 0 3 3 3.5 3.5 0 0 0 3.5 3.5Z" />
      <path d="M7 8h.01M9.5 13h.01M5.5 12h.01" />
    </svg>
  );
}

render(() => <App />, document.getElementById("root")!);
