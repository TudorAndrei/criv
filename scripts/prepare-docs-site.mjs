import { cp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd();
const source = path.join(root, 'docs');
const book = path.join(root, '.site-build', 'docs-book');
const bookSource = path.join(book, 'src');

await rm(book, { recursive: true, force: true });
await mkdir(bookSource, { recursive: true });
await cp(source, bookSource, {
  recursive: true,
  filter: (entry) => !entry.endsWith('.criv'),
});

async function markdownFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await markdownFiles(entryPath));
    if (entry.isFile() && entry.name.endsWith('.md')) files.push(entryPath);
  }
  return files;
}

function withoutFrontmatter(sourceText) {
  if (!sourceText.startsWith('---\n')) return sourceText;
  const end = sourceText.indexOf('\n---\n', 4);
  return end === -1 ? sourceText : sourceText.slice(end + 5);
}

function titleFor(sourceText, relativePath) {
  const frontmatterTitle = sourceText.match(/^---\n[\s\S]*?^title: (.+)$/m)?.[1];
  if (frontmatterTitle) return frontmatterTitle.replace(/^['"]|['"]$/g, '');
  const heading = sourceText.match(/^#\s+(.+)$/m)?.[1];
  return heading ?? path.basename(relativePath, '.md');
}

for (const file of await markdownFiles(bookSource)) {
  const sourceText = await readFile(file, 'utf8');
  await writeFile(file, withoutFrontmatter(sourceText));
}

await mkdir(path.join(bookSource, 'architecture'), { recursive: true });
await writeFile(path.join(bookSource, 'README.md'), `# criv documentation\n\nBrowse the documentation, architecture decisions, and repository guides.\n`);
await writeFile(path.join(bookSource, 'architecture', 'README.md'), `# Architecture\n\nExplore the current C4 model. Select an element for its details, or open the view browser to change views.\n\n<likec4-view view-id="index" browser="true"></likec4-view>\n`);

const chapters = (await markdownFiles(bookSource))
  .map(async (file) => {
    const relativePath = path.relative(bookSource, file).replaceAll(path.sep, '/');
    const sourceText = await readFile(file, 'utf8');
    return { relativePath, title: titleFor(sourceText, relativePath) };
  });
const pages = await Promise.all(chapters);
pages.sort((left, right) => left.relativePath.localeCompare(right.relativePath));

const summary = ['# Summary', '', '[Overview](README.md)', '', '[Architecture](architecture/README.md)', '', '---', ''];
for (const page of pages) {
  if (page.relativePath === 'README.md' || page.relativePath === 'architecture/README.md') continue;
  summary.push(`- [${page.title}](${page.relativePath})`);
}
await writeFile(path.join(bookSource, 'SUMMARY.md'), `${summary.join('\n')}\n`);

await writeFile(path.join(book, 'architecture.css'), `
likec4-view {
  display: block;
  min-height: 42rem;
  margin: 1.5rem 0;
}
`);

await writeFile(path.join(book, 'book.toml'), `
[book]
authors = ["Tudor Andrei"]
language = "en"
src = "src"
title = "criv documentation"

[build]
build-dir = "../../site/docs"

[output.html]
additional-css = ["architecture.css"]
additional-js = ["likec4-webcomponent.js"]
git-repository-icon = "fab-github"
git-repository-url = "https://github.com/TudorAndrei/criv"
site-url = "/criv/docs/"
`);
