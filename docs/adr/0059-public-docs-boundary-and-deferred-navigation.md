# ADR-0059: Public Docs Boundary and Deferred Navigation

- Status: Accepted; docs entry and checks implemented 2026-11-20
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0021](0021-signed-plugin-index.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md)
- Supersedes:
- Superseded by:

## Context

The public site at `anvilkernel.com/docs` is currently a task router. It links
to installation, configuration, LSP, plugins, architecture, and reference
material. The canonical documentation remains the Markdown files in this
repository.

A comparison of established project docs exposed two useful but different
patterns:

- Pi docs use a task-oriented landing page, grouped navigation, a right-hand
  table of contents, search, copy-page and copy-command actions, and
  accessibility metadata.
- Helix docs use an mdBook-style book: numbered chapters, a complete sidebar,
  search, themes, print, edit links, and copy controls on code blocks.

Copying either structure wholesale would be wrong for Anvil today:

- There is only one public docs page. A chapter tree, right TOC, previous/next
  navigation, or search would be empty or misleading.
- Copying repository Markdown into the site would create two sources of truth
  and immediate drift.
- A custom docs framework would add maintenance cost before there is a corpus
  to justify it.
- Public docs must not imply a stable release, signed binaries, or an open
  plugin marketplace when those gates are not met.

The site already has a shared base layout with a skip link, a `main` landmark,
focus-visible styles, non-empty page descriptions, canonical URLs, and
Open Graph/Twitter metadata. Those properties are now part of the docs
contract, not optional page polish.

## Decision

### 1. `/docs` stays a task router until there is a real corpus

Do not add empty chapter pages or a book-style shell just to make the site look
complete. A navigation shell with a grouped sidebar, per-page edit links, and a
right-hand table of contents is added only after there are at least three real
docs pages spanning at least two task groups.

A right-hand TOC is added only to a page with at least three substantive
second-level sections. Short pages keep a single reading column.

### 2. Every docs page uses the shared accessible layout

Every page under `/docs` renders through the site's shared layout and provides:

- a skip-to-content link and a single `main` landmark;
- visible focus styles;
- a non-empty title and description;
- an absolute canonical URL;
- Open Graph and Twitter metadata;
- `aria-label` on icon-only controls and previous/next links.

The site build fails if a docs page is missing these properties. This check is
enforced by `scripts/check-site.mjs` in the site repository.

### 3. Docs content stays source-backed

The repository Markdown remains the source of truth. The site links to it or
renders it from one source; it does not maintain a second hand-edited copy.

`Copy page` and Markdown content negotiation are deferred until docs pages are
backed by a single content collection. Copy controls for code blocks are
required now.

### 4. Search has an explicit threshold

Do not add Pagefind or a custom search dialog for the current one-page docs
entry. Add bounded static search only after at least ten real docs pages exist.

### 5. Version labels are explicit; version selectors are not faked

The docs entry displays the current documentation version and links to the
release/status page. A version selector is added only when multiple maintained
docs versions actually exist.

## Consequences

Positive:

- No empty navigation, dead chapters, or misleading search UI.
- The canonical docs source and the public site cannot drift silently.
- Accessibility and SEO checks are enforced from the first docs page.
- The site keeps the low-maintenance, zero-JS-first docs entry until content
  justifies more machinery.

Negative:

- Docs navigation is deliberately less rich than Pi or Helix until the page
  count grows.
- Readers must use repository links for topics that do not yet have a public
  page.
- The three-page and ten-page thresholds are policy choices and may need
  revision once real usage data exists.

## Verification

- Docs metadata and accessibility: run `python3 scripts/docs-check.py` in this
  repository; run `npm run build` in `anvilkernel-site`, whose generated check
  requires skip links, non-empty descriptions, canonical/Open Graph metadata,
  labeled icon controls, and the docs copy controls.
- Keyboard: Tab from the top reaches the skip link; Enter on a focused copy
  button changes its label to `已复制`.
- Responsive: verify `/docs/` at 390px and desktop widths with no horizontal
  overflow.
- Deployment: the site workflow must build and deploy `/docs/` successfully;
  the current implementation is covered by the site's build-and-deploy run.

## References

- Pi Documentation: <https://docs.pi.dev/latest> — task-router landing page,
  grouped navigation, right-hand table of contents, page actions, and
  accessibility metadata.
- Pi Quickstart: <https://docs.pi.dev/latest/quickstart> — numbered steps,
  code-copy controls, and per-page table of contents.
- Helix Documentation: <https://docs.helix-editor.com/> — mdBook chapter tree,
  search, themes, print, GitHub/edit links, and reference-book navigation.
- Helix Package Managers: <https://docs.helix-editor.com/package-managers.html>
  — long reference page with copy controls on code blocks.
