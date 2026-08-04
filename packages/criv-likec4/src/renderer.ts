import { LikeC4Model } from "likec4/model";
import { LikeC4ModelProvider, ReactLikeC4 } from "likec4/react";
import { createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

import type { CrivLikeC4Model } from "./protocol.js";

export interface CrivLikeC4RendererOptions {
  colorScheme?: "light" | "dark";
  onOpenSource?: (target: string) => void;
  onSelectView?: (viewId: string) => void;
}

export class CrivLikeC4Renderer {
  readonly #container: HTMLElement;
  readonly #options: CrivLikeC4RendererOptions;
  readonly #root: Root;
  #revision = -1;
  #model: CrivLikeC4Model | null = null;
  #viewId: string | null = null;

  constructor(container: HTMLElement, options: CrivLikeC4RendererOptions = {}) {
    this.#container = container;
    this.#options = options;
    this.#root = createRoot(container);
  }

  replace(next: CrivLikeC4Model, viewId?: string): boolean {
    if (next.revision <= this.#revision) {
      return false;
    }
    this.#revision = next.revision;
    this.#model = next;
    // A file that owns no named view renders nothing. There is no fallback view.
    this.#viewId = viewId && next.views.some((view) => view.id === viewId) ? viewId : null;
    this.#render();
    if (this.#viewId) {
      this.#options.onSelectView?.(this.#viewId);
    }
    return true;
  }

  selectView(viewId: string): boolean {
    if (!this.#model?.views.some((view) => view.id === viewId) || this.#viewId === viewId) {
      return false;
    }
    this.#viewId = viewId;
    this.#render();
    this.#options.onSelectView?.(viewId);
    return true;
  }

  currentViewId(): string | null {
    return this.#viewId;
  }

  views(): readonly { id: string; title: string }[] {
    return this.#model?.views ?? [];
  }

  exportSvg(): string | null {
    const host = this.#container.querySelector(".likec4-view") as HTMLElement | null;
    const shadow = host?.shadowRoot;
    if (!host || !shadow) {
      return null;
    }
    const bounds = host.getBoundingClientRect();
    const width = Math.max(1, Math.ceil(bounds.width));
    const height = Math.max(1, Math.ceil(bounds.height));
    return [
      `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
      `<foreignObject width="100%" height="100%">`,
      `<div xmlns="http://www.w3.org/1999/xhtml" style="width:${width}px;height:${height}px">`,
      shadow.innerHTML,
      "</div></foreignObject></svg>",
    ].join("");
  }

  dispose(): void {
    this.#root.unmount();
    this.#model = null;
  }

  #render(): void {
    if (!this.#model || !this.#viewId) {
      this.#root.render(createElement("p", null, "No LikeC4 view is available."));
      return;
    }
    const model = LikeC4Model.create(this.#model.model as never);
    const sourceByElement = new Map<string, string>(
      this.#model.sourceLinks.map((link) => [link.element, link.target] as const),
    );
    this.#root.render(
      createElement(
        LikeC4ModelProvider,
        { likec4model: model },
        createElement(ReactLikeC4, {
          viewId: this.#viewId as never,
          colorScheme: this.#options.colorScheme,
          pannable: true,
          zoomable: true,
          enableSearch: true,
          showNavigationButtons: true,
          onNavigateTo: (viewId: string) => this.selectView(viewId),
          onNodeClick: (event: { node: { id: string; navigateTo?: string | null } }) => {
            if (event.node.navigateTo) {
              return;
            }
            const target = sourceByElement.get(event.node.id);
            if (target) {
              this.#options.onOpenSource?.(target);
            }
          },
          style: { width: "100%", height: "100%" },
        } as never),
      ),
    );
  }
}
