import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const workspace = process.argv[1];
const revision = Number(process.argv[2] ?? 0);
const require = createRequire(new URL(`file://${process.cwd()}/package.json`));

try {
  const entry = require.resolve('likec4');
  const packageJson = JSON.parse(await readFile(join(dirname(dirname(entry)), 'package.json'), 'utf8'));
  const { LikeC4 } = await import(pathToFileURL(entry).href);
  const likec4 = await LikeC4.fromWorkspace(workspace, {
    logger: false,
    printErrors: false,
    throwIfInvalid: false,
  });
  try {
    const diagnostics = likec4.getErrors().map(error => ({
      message: error.message,
      file: error.sourceFsPath,
      line: error.line,
      range: error.range ?? null,
    }));
    const valid = diagnostics.length === 0;
    let model = null;
    if (valid) {
      const data = (await likec4.layoutedModel()).$data;
      const elements = Object.values(data.elements).map(element => ({
        id: element.id,
        kind: element.kind,
        title: element.title,
        parent: element.parent ?? null,
        links: (element.links ?? []).map(link => ({
          title: link.title ?? null,
          url: link.url,
        })),
      })).sort((left, right) => left.id.localeCompare(right.id));
      const relationships = Object.values(data.relations).map(relation => ({
        id: relation.id,
        source: relation.source,
        target: relation.target,
        title: relation.title ?? null,
        kind: relation.kind ?? null,
      })).sort((left, right) => left.id.localeCompare(right.id));
      const views = Object.values(data.views).map(view => ({
        id: view.id,
        title: view.title ?? view.id,
        sourcePath: view.sourcePath,
      })).sort((left, right) => left.id.localeCompare(right.id));
      const normalizeSourceTarget = target => {
        const [path, fragment] = target.split('#', 2);
        if (/^[a-z][a-z0-9+.-]*:/i.test(path)) {
          return target;
        }
        const normalized = relative(process.cwd(), resolve(workspace, path)).replaceAll('\\', '/');
        return fragment ? `${normalized}#${fragment}` : normalized;
      };
      const sourceLinks = elements.flatMap(element => element.links
        .filter(link => link.title?.toLowerCase() === 'source')
        .map(link => ({ element: element.id, target: normalizeSourceTarget(link.url) }))
      ).sort((left, right) => left.element.localeCompare(right.element) || left.target.localeCompare(right.target));
      model = { raw: data, elements, relationships, views, sourceLinks };
    }
    process.stdout.write(JSON.stringify({
      protocolVersion: 1,
      nodeVersion: process.versions.node,
      likec4Version: packageJson.version,
      revision,
      valid,
      diagnostics,
      model,
    }));
  } finally {
    await likec4.dispose();
  }
} catch (error) {
  process.stdout.write(JSON.stringify({
    protocolVersion: 1,
    nodeVersion: process.versions.node,
    likec4Version: 'unknown',
    revision,
    valid: false,
    diagnostics: [],
    model: null,
    bridgeError: String(error?.message ?? error),
  }));
}
