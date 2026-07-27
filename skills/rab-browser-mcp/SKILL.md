---
name: rab-browser-mcp
description: >
  Control the visible tabs of rab-browser through its Attached-mode MCP server.
  Use for navigation, tab management, DOM/text extraction, element interaction,
  and JavaScript evaluation in the GUI browser the user can see.
---

# rab-browser MCP

rab-browser can expose its visible GUI tabs as a local stdio MCP server. Start it
with `--mcp` or set `RAB_MCP=1`. Without either gate, rab-browser starts as a
normal GUI browser and does not occupy stdout.

```bash
cargo run -p browser-app -- --mcp https://example.com
```

An MCP client should launch that command as its stdio server process. The GUI and
MCP server run together; this implementation does not provide a headless mode or
attach to an independently launched process.

## Core workflow

Use:

```text
navigate → list_tabs → get_dom/get_text → click/type/evaluate
```

Tab IDs are stable for the lifetime of the browser process. Tools with an
optional `target` use the active tab when it is omitted.

## Tools

| Tool | Parameters | Result |
|---|---|---|
| `navigate` | `url: string` | `ok`; navigates the active tab |
| `new_tab` | `url?: string` | New numeric tab ID |
| `close_tab` | `id: number` | `true` when the tab existed |
| `select_tab` | `id: number` | `true` when the tab existed |
| `list_tabs` | none | JSON array of `{id,url,title,active}` |
| `go_back` | none | Whether history moved |
| `go_forward` | none | Whether history moved |
| `reload` | none | `ok` |
| `get_dom` | `selector?: string`, `target?: number` | Matching element HTML; defaults to `html` |
| `get_text` | `selector?: string`, `target?: number` | Matching visible text; defaults to `body` |
| `evaluate` | `script: string`, `target?: number` | JSON-serialized JavaScript result |
| `click` | `selector: string`, `target?: number` | `ok` |
| `type` | `selector: string`, `text: string`, `target?: number` | `ok` |

`navigate` accepts a URL, domain-like input, or search text using the same
normalization as the GUI location field. `type` replaces the element value and
dispatches bubbling `input` and `change` events.

## Limits and safety

- This slice has no screenshot or network-request tools.
- Page tools execute JavaScript in the selected page. Treat `evaluate` input as
  code execution in the page's authenticated browser context.
- `evaluate` does not await Promises: wry's evaluation callback cannot resolve a
  returned Promise, so scripts must be synchronous. Passing `async`/`await` code
  returns whatever the outer expression synchronously evaluates to (usually an
  unhelpful Promise object), not the eventually-resolved value.
- Requests time out after 10 seconds if the GUI cannot answer.
- A missing target tab returns an error (or `false` for tab selection/closing).
- MCP protocol data uses stdout; all rab-browser diagnostics use stderr.
