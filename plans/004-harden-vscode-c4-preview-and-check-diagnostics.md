# Plan 004: Deduplicate C4 preview listeners and close three webview/diagnostic hardening gaps

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6295490..HEAD -- extensions/vscode-criv/src extensions/vscode-criv/test/unit`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S–M
- **Risk**: LOW
- **Depends on**: plans/002-wire-vscode-extension-tests-into-the-gate.md
  (soft — the new tests only gate commits once 002 lands; the code changes here
  don't require it)
- **Category**: bug + security
- **Planned at**: commit `6295490`, 2026-07-05

## Why this matters

Four small defects in the VS Code extension's C4 preview and diagnostics path:

1. **Duplicate listeners (user-visible bug)**: every time the preview opens for
   an already-open panel, another `onDidReceiveMessage` handler is registered.
   The preview re-opens on every active-editor change to a `.c4` file, so after
   N refreshes one click on a source button executes the open-source command N
   times.
2. **Weak CSP nonce**: the webview's only allowed `script-src` token is a nonce
   generated with `Math.random()`, which is not cryptographically strong.
3. **Unescaped error sink**: render errors (whose text derives from `.c4` file
   content via Mermaid/Viz error messages) are concatenated into `innerHTML`.
   CSP currently blocks script execution, but the sink shouldn't rely on that.
4. **Unbounded diagnostic paths**: `criv check --format json` output paths are
   joined onto the workspace root with `Uri.joinPath`, which resolves `..` —
   the one file-target path in the extension not routed through the existing
   `safeVaultPath` guard.

All are contained, low-risk fixes in one extension surface.

## Current state

Relevant files (all under `extensions/vscode-criv/`):

- `src/c4Preview.ts` — `C4PreviewManager.open()` creates/reuses the webview
  panel; `nonceValue()` at lines 123–130.
- `src/c4PreviewHtml.ts` — `buildC4PreviewHtml()` produces the full webview
  HTML string, including an inline render script; the error sink is in the
  `catch` near line 85.
- `src/checkDiagnostics.ts` — `CrivCheckDiagnostics.setFromJson()` maps
  parsed diagnostics to URIs; the unbounded join is at line 15.
- `src/sourceTarget.ts` — exports `safeVaultPath(value: unknown): string |
  undefined` (lines 63–90), the existing path guard: trims, converts `\` to
  `/`, rejects absolute/drive/UNC/NUL paths and any `..` segment, returns the
  normalized relative path.
- Tests: `test/unit/c4PreviewHtml.test.ts`, `test/unit/checkDiagnosticModel.test.ts`,
  `test/unit/sourceTarget.test.ts` — `node:test` + `node:assert/strict`,
  importing from `../../src/...`. There is no unit test for `c4Preview.ts`
  (it imports `vscode`, which the unit bundle can't provide — keep it that way).

Excerpt — the listener re-registration (`src/c4Preview.ts:21-40`):

```ts
    const panel =
      this.panel ??
      vscode.window.createWebviewPanel(
        "criv.c4Preview",
        "criv C4 Preview",
        { viewColumn: vscode.ViewColumn.Beside, preserveFocus: options.preserveFocus ?? false },
        {
          enableScripts: true,
          localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
        },
      );
    this.panel = panel;
    panel.onDidDispose(() => {
      this.panel = undefined;
    });
    panel.webview.onDidReceiveMessage(async (message: unknown) => {
      if (isOpenSourceMessage(message)) {
        await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
      }
    });
```

Excerpt — the nonce (`src/c4Preview.ts:123-130`):

```ts
function nonceValue(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let value = "";
  for (let index = 0; index < 32; index += 1) {
    value += alphabet[Math.floor(Math.random() * alphabet.length)] ?? "0";
  }
  return value;
}
```

Excerpt — the error sink inside the webview script template
(`src/c4PreviewHtml.ts:82-87`; note this is inside a template literal, so
backslashes in the file are escaped):

```ts
    diagram.innerHTML = '<div class="error">Unknown .c4 format.</div>';
  } catch (error) {
    diagram.innerHTML = '<div class="error">' + String(error?.message ?? error) + '</div>';
  }
```

Excerpt — the unbounded join (`src/checkDiagnostics.ts:10-15`):

```ts
    for (const item of parseCheckDiagnostics(raw)) {
      if (!item.path) {
        continue;
      }

      const uri = vscode.Uri.joinPath(root, ...item.path.split("/"));
```

Conventions: oxlint + oxfmt (`npm run lint` / `format:check` in the extension
dir); conventional commits (recent example: `fix(vscode): harden command
settings and hovers`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Typecheck + unit tests | `npm --prefix extensions/vscode-criv test` | exit 0 |
| Lint | `npm --prefix extensions/vscode-criv run lint` | exit 0 |
| Format check | `npm --prefix extensions/vscode-criv run format:check` | exit 0 |
| Build | `npm --prefix extensions/vscode-criv run build` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `extensions/vscode-criv/src/c4Preview.ts`
- `extensions/vscode-criv/src/c4PreviewHtml.ts`
- `extensions/vscode-criv/src/checkDiagnostics.ts`
- `extensions/vscode-criv/test/unit/c4PreviewHtml.test.ts`
- `extensions/vscode-criv/test/unit/checkDiagnostics.test.ts` (create only if
  achievable without importing `vscode` — see Step 4; otherwise skip)

**Out of scope** (do NOT touch):
- `.obsidian/plugins/criv/**` — the Obsidian plugin's error rendering is
  already safe (`createDiv({ text })`); its DOT sanitizer `<style>` question is
  a separate investigation, not this plan.
- `src/sourceTarget.ts` — you consume `safeVaultPath`, you don't change it.
- The `sanitizeDotSvg` function inside `c4PreviewHtml.ts` — behavior is
  test-covered for parity with the Obsidian copy; changing it breaks parity.
- `extension.ts` — no wiring changes needed.

## Git workflow

- Conventional commits, one commit per step (or one combined
  `fix(vscode): harden c4 preview and check diagnostics` if the operator
  prefers a single commit).
- Do NOT push unless the operator instructed it.

## Steps

### Step 1: Register panel listeners only on creation

In `C4PreviewManager.open()`, restructure so `onDidDispose` and
`onDidReceiveMessage` are registered exactly once, when the panel is created:

```ts
    let panel = this.panel;
    if (!panel) {
      panel = vscode.window.createWebviewPanel(
        "criv.c4Preview",
        "criv C4 Preview",
        { viewColumn: vscode.ViewColumn.Beside, preserveFocus: options.preserveFocus ?? false },
        {
          enableScripts: true,
          localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
        },
      );
      panel.onDidDispose(() => {
        this.panel = undefined;
      });
      panel.webview.onDidReceiveMessage(async (message: unknown) => {
        if (isOpenSourceMessage(message)) {
          await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
        }
      });
      this.panel = panel;
    }
```

Everything after (relativePath, html assignment, `panel.reveal`) stays as-is.

**Verify**: `npm --prefix extensions/vscode-criv test` → exit 0 (typecheck is
part of `npm test` via `npm run compile`).

### Step 2: Crypto-strong nonce

Replace the body of `nonceValue()` in `c4Preview.ts`:

```ts
import { randomBytes } from "node:crypto";

function nonceValue(): string {
  return randomBytes(16).toString("base64");
}
```

(Extension host code runs in Node; `node:crypto` is available. Base64 is a
valid CSP nonce charset — it may include `+ / =`, which is fine inside the
quoted `'nonce-...'` directive.)

**Verify**: `npm --prefix extensions/vscode-criv test` → exit 0.

### Step 3: Escape the render-error message

In the inline script inside `buildC4PreviewHtml()` (`c4PreviewHtml.ts`),
replace the two `innerHTML` error assignments with DOM construction. Add a
helper inside the same inline script (remember: this code lives in a template
literal — keep the existing escaping style, e.g. `\\` where the current code
uses it):

```js
function showError(text) {
  diagram.textContent = "";
  const node = document.createElement("div");
  node.className = "error";
  node.textContent = text;
  diagram.appendChild(node);
}
```

Then:
- `diagram.innerHTML = '<div class="error">Unknown .c4 format.</div>';`
  becomes `showError("Unknown .c4 format.");`
- the catch body becomes `showError(String(error?.message ?? error));`

Do not touch the two legitimate `diagram.innerHTML = ...` assignments for the
Mermaid render result and the sanitized DOT SVG.

**Verify**: `npm --prefix extensions/vscode-criv test` → exit 0, and the
existing test "keeps source fallback and render-error surface in preview HTML"
still passes (update its assertions if they matched the old literal markup —
check `test/unit/c4PreviewHtml.test.ts` around line 22).

### Step 4: Bound check-diagnostic paths with `safeVaultPath`

In `checkDiagnostics.ts`, import the guard and skip unsafe paths:

```ts
import { safeVaultPath } from "./sourceTarget";
...
    for (const item of parseCheckDiagnostics(raw)) {
      const safePath = safeVaultPath(item.path);
      if (!safePath) {
        continue;
      }

      const uri = vscode.Uri.joinPath(root, ...safePath.split("/"));
```

(The existing `if (!item.path) continue;` is subsumed — `safeVaultPath` returns
`undefined` for empty strings.)

Testing note: `checkDiagnostics.ts` imports `vscode`, so it cannot be unit
tested directly in this suite. `safeVaultPath` itself is already covered in
`test/unit/sourceTarget.test.ts`. Add unit coverage only if you can do it
without importing `vscode` (e.g. if you extract a pure path-mapping helper);
otherwise skip the new test file and rely on typecheck + the guard's existing
tests. Do NOT introduce a vscode mock framework for this.

**Verify**: `npm --prefix extensions/vscode-criv test` → exit 0.

### Step 5: Full gate

**Verify**:
- `npm --prefix extensions/vscode-criv run lint` → exit 0
- `npm --prefix extensions/vscode-criv run format:check` → exit 0
- `npm --prefix extensions/vscode-criv run build` → exit 0
- `git status --short` shows only in-scope files

**Commit** (if not committed per-step):
`fix(vscode): harden c4 preview and check diagnostics`

## Test plan

- Update/extend `test/unit/c4PreviewHtml.test.ts`:
  - assert the built HTML contains the `showError` helper and does NOT contain
    the string `"'<div class=\"error\">' + String"` (the old concatenation);
  - keep the existing CSP assertions passing.
- Nonce and listener changes live in `c4Preview.ts` (imports `vscode`, not
  unit-testable here) — verified by typecheck + build.
- Verification: `npm --prefix extensions/vscode-criv test` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `npm --prefix extensions/vscode-criv test` exits 0
- [ ] `grep -n 'Math.random' extensions/vscode-criv/src/c4Preview.ts` → no matches
- [ ] `grep -c 'onDidReceiveMessage' extensions/vscode-criv/src/c4Preview.ts` → 1,
      and it is inside the panel-creation branch
- [ ] `grep -n "String(error" extensions/vscode-criv/src/c4PreviewHtml.ts` shows
      the error text now flows into `showError(...)`/`textContent`, not `innerHTML`
- [ ] `grep -n 'safeVaultPath' extensions/vscode-criv/src/checkDiagnostics.ts` → 1+ match
- [ ] lint + format:check + build exit 0
- [ ] `git status` clean outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `c4Preview.ts` / `c4PreviewHtml.ts` don't match the excerpts (drift).
- The existing c4PreviewHtml tests assert the exact old error markup in a way
  that suggests an external consumer depends on it (unlikely; but if you find
  a fixture or integration test matching `<div class="error">` from outside
  the unit suite, report).
- Step 4 breaks the integration test (`npm --prefix extensions/vscode-criv run
  test:integration`) — that suite needs a display server; only run it if the
  environment supports it, and report rather than debug failures there.

## Maintenance notes

- The reused-panel branch now skips listener setup; if a future change adds
  more listeners, they must also go inside the creation branch.
- Reviewer: confirm no behavioral change for well-formed diagnostics — paths
  like `docs/index.md` round-trip identically through `safeVaultPath`.
- Deferred: the Obsidian DOT sanitizer `<style>`/CSS investigation (tracked as
  an open finding, not planned); webview state persistence across panel
  disposal.
