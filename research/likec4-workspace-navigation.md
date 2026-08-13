# LikeC4 Workspace and Navigation Research

Date: 2026-08-04

## Question

Can criv split one LikeC4 architecture workspace into many files and folders,
and can users navigate between its focused views?

## Answer

Yes. Keep one LikeC4 project at `docs/architecture/`. Split its model and views
into as many `.c4` files and folders as needed. LikeC4 merges the files into one
model. Use named scoped views and explicit `navigateTo` rules for drill-down.

Criv must also connect LikeC4's `onNavigateTo` event to its selected view. The
current criv renderer does not do this. Moving files will make each opened file
more useful, but moving files alone will not enable diagram navigation.

This research applies to LikeC4 1.59.2, which criv pins. The reviewed official
source is commit
[`67b696e77ef8f97f43e435819a019ff8cc637cec`](https://github.com/likec4/likec4/tree/67b696e77ef8f97f43e435819a019ff8cc637cec).

## One project can use many files

A `likec4.config.json` file makes its folder a project root. LikeC4 includes
`.c4` files in that folder and its subfolders. See the official
[project configuration documentation](https://likec4.dev/dsl/config/), the
[workspace scanner](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/language-server/src/workspace/WorkspaceManager.ts#L61-L91),
and the
[file-system filter](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/language-server/src/filesystem/LikeC4FileSystem.ts#L51-L82).

All source files in one project are merged into one architecture model. An
element can be declared in one file and extended in another file. This is the
intended way to keep one large model in small files. See
[Extending model](https://likec4.dev/dsl/extend/).

Files in the same project do not need `import` statements. The `import` keyword
is for separate LikeC4 projects. A nested `likec4.config.json` starts a separate
project, and its files stop being part of the parent project. Cross-project
imports currently support only top-level elements. Therefore, criv must not use
nested projects only to organize C4 levels. See
[Multi-projects](https://likec4.dev/dsl/config/multi-projects/).

Criv already gives the complete `docs/architecture/` path to
`LikeC4.fromWorkspace`, so a recursive file split fits the current compiler
bridge. See
[`assets/likec4-bridge.mjs`](../assets/likec4-bridge.mjs#L14) and
[`src/likec4.rs`](../src/likec4.rs#L70).

## Source folders and view folders are different

Physical folders organize `.c4` source. The official LikeC4 site can also group
its diagram tree by source file or source folder because every view has a
`sourcePath`. See the official
[diagram tree implementation](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/likec4-spa/src/components/sidebar/data.ts#L41-L104).

The portable DSL feature for navigation folders is the view title. A `/` in a
title creates a folder path. A `views 'Folder name'` block supplies a common
folder for all views in the block. See
[Organize views](https://likec4.dev/dsl/views/organize/).

For example:

```likec4
views 'CLI' {
  view cliComponents of criv.cli {
    title 'Components'
    include *
  }
}
```

The source folder and the title folder should agree. This gives useful
organization in both the official LikeC4 tools and criv hosts.

## Navigation between views

All navigation targets must be named views. A named view has a stable identifier
that LikeC4 can reference and use in a URL. See
[Views](https://likec4.dev/dsl/views/).

### Automatic drill-down with scoped views

A view declared as `view <id> of <element>` becomes a scoped view for that
element. When another view shows the element, LikeC4 can navigate to its default
scoped view. If an element has more than one scoped view, their source order
selects the default. See the official
[scoped-view documentation](https://likec4.dev/dsl/views/#scoped-views) and
[`assignNavigateTo`](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/language-server/src/view-utils/assignNavigateTo.ts#L4-L30).

This is a good fit for the main C4 drill-down path:

```text
System Context -> Containers -> Components
```

The `containers` view should be `of criv`. Each Component view should be `of`
its container. A click on `criv`, `criv.cli`, `criv.vscodeExtension`, or
`criv.obsidianPlugin` can then open the next level.

### Explicit navigation

A view can override an element's target:

```likec4
include criv.cli.refreshPipeline with {
  navigateTo codeRefreshPipeline
}
```

This is useful when one component has many Code views, or when the desired view
is not its first scoped view. See
[custom view navigation](https://likec4.dev/dsl/views/predicates/#with-custom-navigation).

A model relationship can navigate to a dynamic view. A relationship included
in one view can also get a view-specific `navigateTo` target. See
[relationship navigation](https://likec4.dev/dsl/relationships/#navigate-to)
and
[relationship customization](https://likec4.dev/dsl/views/predicates/#relationship-navigation).

Dynamic-view steps can navigate to other dynamic views. This supports a
high-level scenario followed by a detailed scenario. See
[Dynamic view navigation](https://likec4.dev/dsl/views/dynamic/#navigation).

Dynamic views are not required for the normal C4 level path. Scoped element
views are the simpler mechanism for Context, Container, Component, and Code
drill-down.

## What the renderer must do

LikeC4 computes `navigateTo` targets in the layout model, but embedded hosts must
handle the navigation event. `ReactLikeC4` accepts `onNavigateTo`. LikeC4 enables
node and relationship navigation only when this callback exists. It also enables
history buttons only when both `showNavigationButtons` and `onNavigateTo` are
active. See the official
[`LikeC4Diagram` implementation](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/diagram/src/LikeC4Diagram.tsx#L49-L82)
and its
[feature setup](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/diagram/src/LikeC4Diagram.tsx#L149-L179).

The official VS Code extension sends navigation from the webview to the host.
The host updates the current view and sends that view back to the webview. See
the official
[VS Code preview](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/vscode-preview/src/screens/View.tsx#L69-L97)
and
[panel state update](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/vscode/src/panel/useDiagramPanel.ts#L153-L180).

Criv uses `ReactLikeC4`, but it passes no `onNavigateTo` callback. It passes a
`browser` property through an unsafe type cast, but `browser` belongs to
`LikeC4View`, not `ReactLikeC4`. Thus, this property does not provide navigation.
See
[`packages/criv-likec4/src/renderer.ts`](../packages/criv-likec4/src/renderer.ts#L73)
and the official
[`ReactLikeC4` properties](https://github.com/likec4/likec4/blob/67b696e77ef8f97f43e435819a019ff8cc637cec/packages/diagram/src/ReactLikeC4.tsx#L12-L58).

The shared criv renderer needs these changes:

1. Pass `onNavigateTo` to `ReactLikeC4`.
2. Select the requested view and render it.
3. Enable LikeC4 navigation buttons.
4. Notify the host when the selected view changes, so the host view selector
   stays correct.
5. Keep the current source-link action separate from drill-down navigation.

The VS Code host already selects a view from the opened file's `sourcePath`.
This makes one view per source file the safest initial layout. See
[`previewModel.ts`](../extensions/vscode-criv/src/c4/previewModel.ts#L4).

The Obsidian adapter currently opens the first view in the model and does not
select by the opened file's `sourcePath`. It must supply the file-owned view when
the workspace is split. See
[`main.ts`](../.obsidian/plugins/criv/src/main.ts#L750).

## Recommended file layout

Keep one project and use one primary view per view file:

```text
docs/architecture/
  specification.c4
  model/
    landscape.c4
    cli.c4
    vscode.c4
    obsidian.c4
    relationships.c4
  views/
    overview/
      system-context.c4
      containers.c4
    components/
      cli.c4
      vscode.c4
      obsidian.c4
    code/
      cli-vault-loading.c4
      cli-refresh-pipeline.c4
      cli-architecture-exporter.c4
      cli-state-publication.c4
      vscode-preview.c4
      obsidian-adapter.c4
      shared-likec4-renderer.c4
```

Use these title folders:

- `Overview / System context`
- `Overview / Containers`
- `Components / CLI`
- `Components / VS Code`
- `Components / Obsidian`
- `Code / CLI / Vault loading`
- `Code / CLI / Refresh pipeline`
- `Code / CLI / Architecture exporter`
- `Code / CLI / State publication`
- `Code / VS Code / Preview`
- `Code / Obsidian / Adapter`
- `Code / Shared / LikeC4 renderer`

## Recommended implementation order

1. Split the current `.c4` source into the single-project folder structure.
2. Put one primary named view in each view file.
3. Make the Container and Component views scoped.
4. Add explicit Component-to-Code navigation where one target is clear.
5. Add `onNavigateTo` support to the shared renderer.
6. Keep the VS Code and Obsidian selectors synchronized with renderer
   navigation.
7. Select Obsidian's initial view by the opened file's `sourcePath`.
8. Validate the complete workspace with `criv watch --once` and `criv check`.

## Conclusion

The split is supported and is the correct design. Use one LikeC4 project, many
small source files, one primary view per view file, scoped views for C4
drill-down, and explicit `navigateTo` rules for Component-to-Code links. Do not
use nested LikeC4 projects for this purpose.

The split and renderer navigation belong in the same change. Without the
renderer callback, the new view links will exist in the model but will not work
in criv's VS Code or Obsidian previews.
