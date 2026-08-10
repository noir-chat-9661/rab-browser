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
type McpClient =
  | "claude_desktop"
  | "claude_code"
  | "cursor"
  | "windsurf"
  | "cline"
  | "antigravity"
  | "zed"
  | "codex"
  | "opencode";
type SettingsCategory =
  | "language"
  | "search"
  | "appearance"
  | "privacy"
  | "performance"
  | "mcp";

type BrowserState = {
  type: "state";
  tabs: Tab[];
  currentTabId: number | null;
  bookmarks: Bookmark[];
  mcpEnabled: boolean;
  mcpHttp: {
    enabled: boolean;
    port: number;
    error: string | null;
  };
  mcpRegistration: {
    registered: McpClient[];
    errors: { client: string; message: string }[];
  } | null;
  settings: {
    searchEngine: SearchEngine;
    theme: Theme;
    locale: Locale;
    tabSuspendGraceSecs: number;
  };
};

type ChromeApi = {
  receive: (state: BrowserState) => void;
  openLocation: () => void;
  openSettings: () => void;
  openMcpHelp: () => void;
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
  mcpEnabled: false,
  mcpHttp: {
    enabled: false,
    port: 8765,
    error: null,
  },
  mcpRegistration: null,
  settings: {
    searchEngine: "google",
    theme: "dark",
    locale: "japanese",
    tabSuspendGraceSecs: 300,
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

const mcpClients: McpClient[] = [
  "claude_desktop",
  "claude_code",
  "cursor",
  "windsurf",
  "cline",
  "antigravity",
  "zed",
  "codex",
  "opencode",
];

function App() {
  const [state, setState] = createSignal(emptyState);
  const [locationOpen, setLocationOpen] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [settingsCategory, setSettingsCategory] =
    createSignal<SettingsCategory>("language");
  const [locationValue, setLocationValue] = createSignal("");
  const [mcpHttpPort, setMcpHttpPort] = createSignal("8765");
  const [mcpHttpPortInvalid, setMcpHttpPortInvalid] = createSignal(false);
  const [mcpRegistrationOpen, setMcpRegistrationOpen] = createSignal(false);
  const [selectedMcpClients, setSelectedMcpClients] =
    createSignal<McpClient[]>([]);
  const [tabSuspendGraceMinutes, setTabSuspendGraceMinutes] = createSignal("5");
  const [tabSuspendGraceInvalid, setTabSuspendGraceInvalid] = createSignal(false);
  const [confirmDialog, setConfirmDialog] = createSignal<{
    message: string;
    onConfirm: () => void;
  } | null>(null);
  let locationInput: HTMLInputElement | undefined;
  let settingsCloseButton: HTMLButtonElement | undefined;
  let settingsContentPanel: HTMLDivElement | undefined;
  let confirmCancelButton: HTMLButtonElement | undefined;
  let confirmOkButton: HTMLButtonElement | undefined;
  let confirmDialogPreviouslyFocused: HTMLElement | null = null;
  let mcpRegistrationDialog: HTMLElement | undefined;
  let mcpRegistrationFirstCheckbox: HTMLInputElement | undefined;
  let mcpRegistrationPreviouslyFocused: HTMLElement | null = null;
  const t = createMemo(() => translations[state().settings.locale]);

  // wry's WKUIDelegate does not implement the JS confirm/alert panels, so
  // window.confirm() silently no-ops on macOS. Use an in-app modal instead.
  const requestConfirm = (message: string, onConfirm: () => void) => {
    confirmDialogPreviouslyFocused = document.activeElement as HTMLElement | null;
    setConfirmDialog({ message, onConfirm });
    // Default focus to Cancel, not the destructive action, so an accidental
    // Enter/Space press doesn't confirm the deletion.
    queueMicrotask(() => confirmCancelButton?.focus());
  };
  const closeConfirmDialog = () => {
    setConfirmDialog(null);
    confirmDialogPreviouslyFocused?.focus();
    confirmDialogPreviouslyFocused = null;
  };
  const acceptConfirmDialog = () => {
    const dialog = confirmDialog();
    if (!dialog) return;
    closeConfirmDialog();
    dialog.onConfirm();
  };
  const trapConfirmDialogTab = (event: KeyboardEvent) => {
    // Only two focusable elements in this dialog, so Tab and Shift+Tab both
    // just toggle between them, keeping focus from escaping to the page
    // behind the modal.
    if (event.key !== "Tab") return;
    event.preventDefault();
    (document.activeElement === confirmCancelButton
      ? confirmOkButton
      : confirmCancelButton
    )?.focus();
  };

  const openMcpRegistration = () => {
    mcpRegistrationPreviouslyFocused = document.activeElement as HTMLElement | null;
    setSelectedMcpClients([...mcpClients]);
    setMcpRegistrationOpen(true);
    queueMicrotask(() => mcpRegistrationFirstCheckbox?.focus());
  };
  const closeMcpRegistration = () => {
    setMcpRegistrationOpen(false);
    mcpRegistrationPreviouslyFocused?.focus();
    mcpRegistrationPreviouslyFocused = null;
  };
  const toggleMcpClient = (client: McpClient, checked: boolean) => {
    setSelectedMcpClients((selected) =>
      checked
        ? [...selected, client]
        : selected.filter((candidate) => candidate !== client),
    );
  };
  const registerMcpClients = () => {
    const clients = selectedMcpClients();
    if (clients.length === 0) return;
    send({ type: "register_mcp_clients", clients });
    closeMcpRegistration();
  };
  const trapMcpRegistrationTab = (event: KeyboardEvent) => {
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      mcpRegistrationDialog?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled)',
      ) ?? [],
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const selectSettingsCategory = (category: SettingsCategory) => {
    setSettingsCategory(category);
    // Move focus into the newly shown panel so switching category doesn't
    // strand keyboard/screen-reader focus on an unmounted element.
    queueMicrotask(() => {
      settingsContentPanel?.focus();
    });
  };

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
  const mcpHttpEndpoint = createMemo(
    () => `http://127.0.0.1:${mcpHttpPort()}/mcp`,
  );

  const validMcpHttpPort = () => {
    const port = Number(mcpHttpPort());
    return Number.isInteger(port) && port >= 1024 && port <= 65535
      ? port
      : null;
  };

  const updateMcpHttp = (enabled: boolean) => {
    if (!enabled) {
      // Disabling must always go through, even if the port field currently
      // holds a draft/invalid value — otherwise a running server could be
      // stranded with no way to turn it off from the UI.
      setMcpHttpPortInvalid(false);
      send({ type: "set_mcp_http", enabled: false, port: state().mcpHttp.port });
      return;
    }
    const port = validMcpHttpPort();
    setMcpHttpPortInvalid(port === null);
    if (port !== null) send({ type: "set_mcp_http", enabled, port });
  };

  const validTabSuspendGraceMinutes = () => {
    const minutes = Number(tabSuspendGraceMinutes());
    return Number.isInteger(minutes) && minutes >= 1 && minutes <= 60
      ? minutes
      : null;
  };

  const updateTabSuspendGrace = () => {
    const minutes = validTabSuspendGraceMinutes();
    setTabSuspendGraceInvalid(minutes === null);
    if (minutes !== null) send({ type: "set_tab_suspend_grace", secs: minutes * 60 });
  };

  createEffect(() => {
    document.documentElement.dataset.theme = state().settings.theme;
    document.documentElement.lang =
      state().settings.locale === "japanese" ? "ja" : "en";
  });

  createEffect(() => {
    setMcpHttpPort(String(state().mcpHttp.port));
  });

  createEffect(() => {
    const secs = state().settings.tabSuspendGraceSecs;
    // Skip resync while the draft is mid-edit and invalid, so an unrelated
    // state broadcast (e.g. a tab title update) doesn't wipe out what the
    // user is currently typing before they've entered a valid value.
    if (tabSuspendGraceInvalid()) return;
    setTabSuspendGraceMinutes(String(Math.round(secs / 60)));
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

  const openMcpHelp = () => {
    setSettingsCategory("mcp");
    openSettings();
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
      openMcpHelp,
    };
    send({ type: "chrome_ready" });

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        if (mcpRegistrationOpen()) {
          event.preventDefault();
          closeMcpRegistration();
          return;
        }
        if (confirmDialog()) {
          event.preventDefault();
          closeConfirmDialog();
          return;
        }
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
          <button
            class="new-tab"
            type="button"
            onClick={() => send({ type: "new_tab" })}
          >
            <Plus />
            <span>{t().newTab}</span>
            <kbd>{shortcutLabel("T")}</kbd>
          </button>
          <div class="tab-list">
            <For each={state().tabs}>
              {(tab) => (
                <button
                  class="tab"
                  classList={{ active: tab.id === state().currentTabId }}
                  type="button"
                  title={isNewTabUrl(tab.url) ? t().readyToBrowse : tab.url}
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
                  <span class="tab-title">{displayTitle(tab, t().newTab)}</span>
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
            <div class="settings-layout">
              <nav class="settings-nav" role="tablist" aria-label={t().settings}>
                <For
                  each={[
                    { id: "language" as const, label: t().language },
                    { id: "search" as const, label: t().search },
                    { id: "appearance" as const, label: t().appearance },
                    { id: "privacy" as const, label: t().privacy },
                    { id: "performance" as const, label: t().performance },
                    { id: "mcp" as const, label: t().mcp },
                  ]}
                >
                  {(category, index) => (
                    <button
                      class="settings-nav-item"
                      classList={{ active: settingsCategory() === category.id }}
                      type="button"
                      role="tab"
                      id={`settings-tab-${category.id}`}
                      aria-selected={settingsCategory() === category.id}
                      aria-controls="settings-content-panel"
                      onClick={() => selectSettingsCategory(category.id)}
                    >
                      <span class="settings-nav-index">
                        {(index() + 1).toString().padStart(2, "0")}
                      </span>
                      <span>{category.label}</span>
                    </button>
                  )}
                </For>
              </nav>

              <div
                class="settings-content"
                id="settings-content-panel"
                role="tabpanel"
                tabindex="-1"
                aria-labelledby={`settings-tab-${settingsCategory()}`}
                ref={settingsContentPanel}
              >
                <Show when={settingsCategory() === "language"}>
                  <section class="settings-category-panel">
                    <header class="settings-category-header">
                      <h2>{t().language}</h2>
                      <p>{t().languageDescription}</p>
                    </header>
                    <div class="theme-options">
                      <For each={locales}>
                        {(locale) => (
                          <label
                            class="theme-option"
                            classList={{ selected: state().settings.locale === locale }}
                          >
                            <input
                              type="radio"
                              name="locale"
                              value={locale}
                              checked={state().settings.locale === locale}
                              onChange={() => send({ type: "set_locale", locale })}
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
                  </section>
                </Show>

                <Show when={settingsCategory() === "search"}>
                  <section class="settings-category-panel">
                    <header class="settings-category-header">
                      <h2>{t().defaultSearchEngine}</h2>
                      <p>{t().defaultSearchEngineDescription}</p>
                    </header>
                    <div class="search-engine-options">
                      <For each={searchEngines}>
                        {(engine) => (
                          <label
                            class="search-engine-option"
                            classList={{ selected: state().settings.searchEngine === engine.value }}
                          >
                            <input
                              type="radio"
                              name="search-engine"
                              value={engine.value}
                              checked={state().settings.searchEngine === engine.value}
                              onChange={() =>
                                send({ type: "set_search_engine", engine: engine.value })
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
                  </section>
                </Show>

                <Show when={settingsCategory() === "appearance"}>
                  <section class="settings-category-panel">
                    <header class="settings-category-header">
                      <h2>{t().appearance}</h2>
                      <p>{t().appearanceDescription}</p>
                    </header>
                    <div class="theme-options">
                      <For each={themes}>
                        {(theme) => (
                          <label
                            class="theme-option"
                            classList={{ selected: state().settings.theme === theme }}
                          >
                            <input
                              type="radio"
                              name="theme"
                              value={theme}
                              checked={state().settings.theme === theme}
                              onChange={() => send({ type: "set_theme", theme })}
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
                  </section>
                </Show>

                <Show when={settingsCategory() === "privacy"}>
                  <section class="settings-category-panel">
                    <header class="settings-category-header">
                      <h2>{t().privacy}</h2>
                      <p>{t().privacyDescription}</p>
                    </header>
                    <div class="privacy-actions">
                      <button
                        class="privacy-action"
                        type="button"
                        onClick={() => {
                          requestConfirm(t().clearHistoryConfirm, () =>
                            send({ type: "clear_history" }),
                          );
                        }}
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
                        onClick={() => {
                          requestConfirm(t().clearCookiesConfirm, () =>
                            send({ type: "clear_cookies" }),
                          );
                        }}
                      >
                        <span>
                          <strong>{t().clearCookies}</strong>
                          <small>{t().clearCookiesDescription}</small>
                        </span>
                        <Cookie />
                      </button>
                    </div>
                  </section>
                </Show>

                <Show when={settingsCategory() === "performance"}>
                  <section class="settings-category-panel">
                    <header class="settings-category-header">
                      <h2>{t().performance}</h2>
                      <p>{t().performanceDescription}</p>
                    </header>
                    <label class="mcp-http-port tab-suspend-grace">
                      <span>{t().tabSuspendGrace}</span>
                      <input
                        type="number"
                        min="1"
                        max="60"
                        value={tabSuspendGraceMinutes()}
                        aria-invalid={tabSuspendGraceInvalid()}
                        onInput={(event) => {
                          setTabSuspendGraceMinutes(event.currentTarget.value);
                          updateTabSuspendGrace();
                        }}
                      />
                      <span>{t().tabSuspendGraceUnit}</span>
                    </label>
                    <p class="settings-hint">{t().tabSuspendGraceDescription}</p>
                    <Show when={tabSuspendGraceInvalid()}>
                      <p class="mcp-http-error" role="alert">
                        {t().tabSuspendGraceInvalid}
                      </p>
                    </Show>
                  </section>
                </Show>

                <Show when={settingsCategory() === "mcp"}>
                  <section class="settings-category-panel mcp-settings">
                    <header class="settings-category-header">
                      <h2>{t().mcp}</h2>
                      <p>{t().mcpDescription}</p>
                    </header>
                    <p class="mcp-overview">{t().mcpOverview}</p>
                    <div class="mcp-info-grid">
                      <div class="mcp-info-block">
                        <h3>{t().mcpStdioMode}</h3>
                        <p>{t().mcpEnablementDescription}</p>
                        <div class="mcp-command-list">
                          <code>rab-browser --mcp</code>
                          <code>RAB_MCP=1 rab-browser</code>
                        </div>
                      </div>
                      <div class="mcp-info-block">
                        <h3>{t().mcpStatus}</h3>
                        <div
                          class="mcp-status"
                          classList={{ enabled: state().mcpEnabled }}
                        >
                          <span class="mcp-status-dot" aria-hidden="true" />
                          <strong>
                            {state().mcpEnabled ? t().mcpEnabled : t().mcpDisabled}
                          </strong>
                        </div>
                        <p>
                          {state().mcpEnabled
                            ? t().mcpEnabledDescription
                            : t().mcpDisabledDescription}
                        </p>
                      </div>
                    </div>
                    <div class="mcp-registration-block">
                      <div>
                        <h3>{t().mcpRegisterClients}</h3>
                        <p>{t().mcpRegisterDescription}</p>
                      </div>
                      <button type="button" onClick={openMcpRegistration}>
                        {t().mcpRegisterButton}
                      </button>
                      <Show when={state().mcpRegistration}>
                        {(registration) => (
                          <div class="mcp-registration-feedback">
                            <Show when={registration().registered.length > 0}>
                              <p class="mcp-registration-success" role="status">
                                <strong>{t().mcpRegisterSuccess}:</strong>{" "}
                                {registration()
                                  .registered.map((client) => t().mcpClients[client])
                                  .join(", ")}
                              </p>
                            </Show>
                            <For each={registration().errors}>
                              {(error) => (
                                <p class="mcp-http-error" role="alert">
                                  <strong>
                                    {t().mcpRegisterError} ({
                                      error.client in t().mcpClients
                                        ? t().mcpClients[
                                            error.client as McpClient
                                          ]
                                        : error.client
                                    }):
                                  </strong>{" "}
                                  {error.message}
                                </p>
                              )}
                            </For>
                          </div>
                        )}
                      </Show>
                    </div>
                    <div class="mcp-http-block">
                      <div class="mcp-http-heading">
                        <div>
                          <h3>{t().mcpHttpToggle}</h3>
                          <p>{t().mcpHttpDescription}</p>
                        </div>
                        <label class="toggle-switch">
                          <input
                            type="checkbox"
                            checked={state().mcpHttp.enabled}
                            aria-label={t().mcpHttpToggle}
                            onChange={(event) =>
                              updateMcpHttp(event.currentTarget.checked)
                            }
                          />
                          <span aria-hidden="true" />
                        </label>
                      </div>
                      <label class="mcp-http-port">
                        <span>{t().mcpHttpPort}</span>
                        <input
                          type="number"
                          min="1024"
                          max="65535"
                          value={mcpHttpPort()}
                          aria-invalid={mcpHttpPortInvalid()}
                          onInput={(event) => {
                            setMcpHttpPort(event.currentTarget.value);
                            if (state().mcpHttp.enabled) {
                              updateMcpHttp(true);
                            } else {
                              setMcpHttpPortInvalid(validMcpHttpPort() === null);
                            }
                          }}
                        />
                      </label>
                      <Show when={mcpHttpPortInvalid()}>
                        <p class="mcp-http-error" role="alert">
                          {t().mcpHttpPortInvalid}
                        </p>
                      </Show>
                      <div class="mcp-http-endpoint">
                        <span>{t().mcpHttpEndpoint}</span>
                        <code>{mcpHttpEndpoint()}</code>
                      </div>
                      <div class="mcp-command-list">
                        <code>
                          {`claude mcp add --transport http rab-browser ${mcpHttpEndpoint()}`}
                        </code>
                      </div>
                      <Show when={state().mcpHttp.error}>
                        {(error) => (
                          <p class="mcp-http-error" role="alert">
                            <strong>{t().mcpHttpError}:</strong> {error()}
                          </p>
                        )}
                      </Show>
                    </div>
                    <div class="mcp-tools-section">
                      <h3>{t().mcpAvailableTools}</h3>
                      <ul class="mcp-tool-list">
                        <For
                          each={[
                            ["navigate", t().mcpTools.navigate],
                            ["new_tab", t().mcpTools.newTab],
                            ["close_tab", t().mcpTools.closeTab],
                            ["select_tab", t().mcpTools.selectTab],
                            ["list_tabs", t().mcpTools.listTabs],
                            ["go_back", t().mcpTools.goBack],
                            ["go_forward", t().mcpTools.goForward],
                            ["reload", t().mcpTools.reload],
                            ["get_dom", t().mcpTools.getDom],
                            ["get_text", t().mcpTools.getText],
                            ["evaluate", t().mcpTools.evaluate],
                            ["click", t().mcpTools.click],
                            ["type", t().mcpTools.type],
                          ]}
                        >
                          {(tool) => (
                            <li>
                              <code>{tool[0]}</code>
                              <span>{tool[1]}</span>
                            </li>
                          )}
                        </For>
                      </ul>
                    </div>
                  </section>
                </Show>
              </div>
            </div>
          </section>
        </div>
      </Show>

      <Show when={mcpRegistrationOpen()}>
        <div class="palette-backdrop confirm-backdrop" onClick={closeMcpRegistration}>
          <section
            ref={mcpRegistrationDialog}
            class="confirm-dialog mcp-registration-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="mcp-registration-title"
            aria-describedby="mcp-registration-description"
            onKeyDown={trapMcpRegistrationTab}
            onClick={(event) => event.stopPropagation()}
          >
            <h2 id="mcp-registration-title">{t().mcpRegisterDialogTitle}</h2>
            <p id="mcp-registration-description">
              {t().mcpRegisterDialogDescription}
            </p>
            <div class="mcp-client-options">
              <For each={mcpClients}>
                {(client) => (
                  <label>
                    <input
                      ref={(element) => {
                        if (client === "claude_desktop") {
                          mcpRegistrationFirstCheckbox = element;
                        }
                      }}
                      type="checkbox"
                      checked={selectedMcpClients().includes(client)}
                      onChange={(event) =>
                        toggleMcpClient(client, event.currentTarget.checked)
                      }
                    />
                    <span>{t().mcpClients[client]}</span>
                  </label>
                )}
              </For>
            </div>
            <div class="confirm-dialog-actions">
              <button
                type="button"
                class="confirm-dialog-cancel"
                onClick={closeMcpRegistration}
              >
                {t().mcpRegisterCancel}
              </button>
              <button
                type="button"
                class="confirm-dialog-ok mcp-registration-submit"
                disabled={selectedMcpClients().length === 0}
                onClick={registerMcpClients}
              >
                {t().mcpRegisterSubmit}
              </button>
            </div>
          </section>
        </div>
      </Show>

      <Show when={confirmDialog()}>
        {(dialog) => (
          <div class="palette-backdrop confirm-backdrop" onClick={closeConfirmDialog}>
            <section
              class="confirm-dialog"
              role="alertdialog"
              aria-modal="true"
              aria-label={t().confirmDialogTitle}
              aria-describedby="confirm-dialog-message"
              onKeyDown={trapConfirmDialogTab}
              onClick={(event) => event.stopPropagation()}
            >
              <p id="confirm-dialog-message">{dialog().message}</p>
              <div class="confirm-dialog-actions">
                <button
                  ref={confirmCancelButton}
                  type="button"
                  class="confirm-dialog-cancel"
                  onClick={closeConfirmDialog}
                >
                  {t().confirmDialogCancel}
                </button>
                <button
                  ref={confirmOkButton}
                  type="button"
                  class="confirm-dialog-ok"
                  onClick={acceptConfirmDialog}
                >
                  {t().confirmDialogOk}
                </button>
              </div>
            </section>
          </div>
        )}
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
