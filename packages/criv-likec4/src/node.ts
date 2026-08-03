import { LikeC4 } from "likec4";

import {
  CRIV_LIKEC4_PROTOCOL_VERSION,
  CRIV_LIKEC4_NODE_VERSION,
  CRIV_LIKEC4_VERSION,
  type CrivLikeC4Model,
} from "./protocol.js";

export async function buildLikeC4Model(
  source: string,
  revision = 0,
): Promise<CrivLikeC4Model> {
  const likec4 = await LikeC4.fromSource(source, {
    logger: false,
    printErrors: false,
    throwIfInvalid: false,
  });
  try {
    const errors = likec4.getErrors();
    if (errors.length > 0) {
      throw new Error(errors.map((error) => error.message).join("\n"));
    }
    const model = (await likec4.layoutedModel()).$data;
    const views = Object.values(model.views)
      .map((view) => ({ id: view.id, title: view.title ?? view.id }))
      .sort((left, right) => left.id.localeCompare(right.id));
    const sourceLinks = Object.values(model.elements)
      .flatMap((element) =>
        (element.links ?? [])
          .filter((link) => link.title?.toLowerCase() === "source")
          .map((link) => ({ element: element.id, target: link.url })),
      )
      .sort((left, right) =>
        left.element.localeCompare(right.element) || left.target.localeCompare(right.target),
      );
    return {
      protocolVersion: CRIV_LIKEC4_PROTOCOL_VERSION,
      nodeVersion: CRIV_LIKEC4_NODE_VERSION,
      likec4Version: CRIV_LIKEC4_VERSION,
      revision,
      model,
      views,
      sourceLinks,
    };
  } finally {
    await likec4.dispose();
  }
}
