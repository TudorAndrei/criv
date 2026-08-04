import { mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd();
const source = path.join(root, 'docs');
const book = path.join(root, '.site-build', 'docs-book');
const bookSource = path.join(book, 'src');

await rm(book, { recursive: true, force: true });
await mkdir(bookSource, { recursive: true });

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

function frontmatter(sourceText) {
  if (!sourceText.startsWith('---\n')) return '';
  const end = sourceText.indexOf('\n---\n', 4);
  return end === -1 ? '' : sourceText.slice(4, end);
}

function withoutFrontmatter(sourceText) {
  if (!sourceText.startsWith('---\n')) return sourceText;
  const end = sourceText.indexOf('\n---\n', 4);
  return end === -1 ? sourceText : sourceText.slice(end + 5);
}

function field(sourceText, name) {
  return frontmatter(sourceText).match(new RegExp(`^${name}: (.+)$`, 'm'))?.[1]
    ?.replace(/^['"]|['"]$/g, '');
}

function titleFor(sourceText, relativePath) {
  const frontmatterTitle = field(sourceText, 'title');
  if (frontmatterTitle) return frontmatterTitle.replace(/^['"]|['"]$/g, '');
  const heading = sourceText.match(/^#\s+(.+)$/m)?.[1];
  return heading ?? path.basename(relativePath, '.md');
}

const documents = [];
for (const file of await markdownFiles(source)) {
  const sourceText = await readFile(file, 'utf8');
  if (field(sourceText, 'kind') !== 'doc') continue;
  const relativePath = path.relative(source, file).replaceAll(path.sep, '/');
  documents.push({
    file,
    relativePath,
    sourceText,
    id: field(sourceText, 'id'),
  });
}

const documentsByTarget = new Map();
for (const document of documents) {
  const basename = path.basename(document.relativePath, '.md');
  for (const target of [document.id, basename]) {
    if (target) documentsByTarget.set(target, document);
  }
}

function convertWikiLinks(sourceText, currentDocument) {
  return sourceText.replace(/\[\[([^\]|#]+)(?:#[^\]|]+)?(?:\|([^\]]+))?\]\]/g, (_match, target, label) => {
    const text = label ?? target;
    const destination = documentsByTarget.get(target);
    if (!destination) return text;
    let relativePath = path.relative(path.dirname(currentDocument.relativePath), destination.relativePath)
      .replaceAll(path.sep, '/');
    if (!relativePath.startsWith('.')) relativePath = `./${relativePath}`;
    return `[${text}](${relativePath})`;
  });
}

for (const document of documents) {
  const outputPath = path.join(bookSource, document.relativePath);
  await mkdir(path.dirname(outputPath), { recursive: true });
  const output = convertWikiLinks(withoutFrontmatter(document.sourceText), document);
  await writeFile(outputPath, output);
}

await writeFile(path.join(bookSource, 'README.md'), `# criv documentation\n\nBrowse the documentation, architecture decisions, and repository guides.\n`);

const chapters = (await markdownFiles(bookSource))
  .map(async (file) => {
    const relativePath = path.relative(bookSource, file).replaceAll(path.sep, '/');
    const sourceText = await readFile(file, 'utf8');
    return { relativePath, title: titleFor(sourceText, relativePath) };
  });
const pages = await Promise.all(chapters);
pages.sort((left, right) => left.relativePath.localeCompare(right.relativePath));

const summary = ['# Summary', '', '[Overview](README.md)', '', '---', ''];
for (const page of pages) {
  if (page.relativePath === 'README.md') continue;
  summary.push(`- [${page.title}](${page.relativePath})`);
}
await writeFile(path.join(bookSource, 'SUMMARY.md'), `${summary.join('\n')}\n`);

await writeFile(path.join(book, 'architecture.css'), `
:root {
  --site-bg: #fff;
  --site-fg: #1c1c1c;
  --site-muted: #6b6b6b;
  --site-rule: #d8d8d8;
  --site-subtle: #f7f7f7;
  --site-focus: #1c1c1c;
  --content-max-width: 68ch;
  --page-padding: 1.25rem;
  --menu-bar-height: 3.5rem;
  --mono-font: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  --code-font-size: 0.95em;
}

html,
body,
#mdbook-sidebar,
#mdbook-menu-bar,
.page-wrapper {
  font-family: var(--mono-font);
  color: var(--site-fg);
  background: var(--site-bg);
}

html {
  font-size: 100%;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

body {
  font-size: 15px;
  line-height: 1.6;
}

.light,
.rust,
.coal,
.navy,
.ayu,
html:not(.js) {
  --bg: var(--site-bg);
  --fg: var(--site-fg);
  --sidebar-bg: var(--site-bg);
  --sidebar-fg: var(--site-fg);
  --sidebar-non-existant: var(--site-muted);
  --sidebar-active: var(--site-fg);
  --sidebar-spacer: var(--site-rule);
  --scrollbar: var(--site-muted);
  --icons: var(--site-fg);
  --icons-hover: var(--site-fg);
  --links: var(--site-fg);
  --inline-code-color: var(--site-fg);
  --theme-popup-bg: var(--site-bg);
  --theme-popup-border: var(--site-rule);
  --theme-hover: var(--site-subtle);
  --quote-bg: var(--site-subtle);
  --quote-border: var(--site-rule);
  --table-border-color: var(--site-rule);
  --table-header-bg: var(--site-subtle);
  --table-alternate-bg: var(--site-subtle);
  --searchbar-border-color: var(--site-rule);
  --searchbar-bg: var(--site-bg);
  --searchbar-fg: var(--site-fg);
  --searchbar-shadow-color: transparent;
  --searchresults-header-fg: var(--site-muted);
  --searchresults-border-color: var(--site-rule);
  --searchresults-li-bg: var(--site-subtle);
  --search-mark-bg: #e8e8e8;
  --color-scheme: light;
}

#mdbook-menu-bar {
  border-block-end: 1px solid var(--site-rule);
}

.menu-title {
  font: inherit;
  font-weight: 700;
  font-size: 1rem;
  text-align: start;
}

.right-buttons,
.left-buttons {
  margin-inline: 0.25rem;
}

#mdbook-menu-bar .fa-svg,
#mdbook-menu-bar .icon-button {
  padding-inline: 0.55rem;
}

#mdbook-sidebar {
  border-inline-end: 1px solid var(--site-rule);
}

#mdbook-sidebar a {
  color: var(--site-fg);
  text-decoration: none;
}

#mdbook-sidebar a:hover,
#mdbook-sidebar a.active {
  text-decoration: underline;
  text-underline-offset: 0.18em;
}

.content {
  padding-block: 2.5rem 4rem;
}

.content main {
  max-width: var(--content-max-width);
}

.content p,
.content ol,
.content ul {
  line-height: 1.6;
}

.content h1,
.content h2,
.content h3,
.content h4,
.content h5,
.content h6 {
  font: inherit;
  font-weight: 700;
  line-height: 1.25;
  margin-block: 2rem 0.75rem;
  text-wrap: balance;
}

.content h1 { margin-block-start: 0; }
.content h3,
.content h4,
.content h5,
.content h6 { margin-block-start: 1.5rem; }

.content a,
.content a:visited,
#mdbook-searchresults a {
  color: var(--site-fg);
  text-decoration: underline;
  text-decoration-thickness: from-font;
  text-underline-offset: 0.18em;
}

.content .header:link,
.content .header:visited {
  color: var(--site-fg);
}

pre {
  margin-block: 0 1rem;
  padding: 0.5rem 0 0.5rem 2ch;
  border-inline-start: 1px solid var(--site-rule);
  background: transparent;
}

pre > code {
  padding: 0;
}

:not(pre) > code {
  padding: 0.1em 0.25em;
  background: var(--site-subtle);
  border-radius: 0;
}

blockquote {
  margin: 1rem 0;
  padding: 0.1rem 0 0.1rem 2ch;
  border: 0;
  border-inline-start: 1px solid var(--site-rule);
  background: transparent;
}

table {
  margin-inline: 0;
}

table td,
table th {
  padding: 0.45rem 0.75rem;
}

#mdbook-searchbar {
  border-radius: 0;
  font: inherit;
}

:focus-visible {
  outline: 2px solid var(--site-focus);
  outline-offset: 3px;
}

@media (prefers-color-scheme: dark) {
  :root {
    --site-bg: #101010;
    --site-fg: #d8d8d8;
    --site-muted: #a0a0a0;
    --site-rule: #2e2e2e;
    --site-subtle: #181818;
    --site-focus: #d8d8d8;
  }

  .light,
  .rust,
  .coal,
  .navy,
  .ayu,
  html:not(.js) {
    --color-scheme: dark;
  }
}

@media only screen and (max-width: 1080px) {
  .content {
    padding-block-start: 2rem;
  }

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
git-repository-icon = "fab-github"
git-repository-url = "https://github.com/TudorAndrei/criv"
site-url = "/criv/docs/"
`);
