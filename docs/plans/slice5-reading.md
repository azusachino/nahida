# Slice 5: anchored reading and notes

Status: locally verified; ready for PR review.

## Checkpoint and scope

The owner requested "merge it and next one" after slice 4. PR #28 merged at
`bca4cd952401848fa02778a69926a92ab4ea2ee4`; its writing implementation is
`d12cbda2603827ea8dd4a3dfb0189e07b2d0e925`. This slice starts from that merge
on `feat/reading-pack`. One writer, this session. Workstation-task playbook,
tier 1; [criterion 12](../specs/ts-rework.md) owns acceptance.

Implement local PDF extraction with page anchors, public HTTPS HTML extraction
with section anchors, and highlights/notes saved to workspace Markdown. EPUB
is explicitly deferred under the spec's allowance. No OCR, authenticated sites,
JavaScript rendering, local HTML import, remote PDFs, live inference, release
or installation. Gates: `make check` before commit, `make validate` before PR.

## Dependencies and sources

No PDF utility is installed in the shared toolset. Use exactly pinned project
libraries rather than install a host utility or write PDF/HTML/IP parsers:

- `unpdf` 2.1.0: [official extraction API](https://github.com/unjs/unpdf#extract-text),
  plus its installed README and `dist/index.{mjs,d.mts}`. It bundles PDF.js
  6.4.299 and supports Bun, including per-page text and resource-limit options.
- `linkedom` 0.18.15: [official DOM parser API](https://github.com/WebReflection/linkedom#readme),
  plus its installed README/types. Parsing does not run page scripts or fetch
  subresources.
- `ipaddr.js` 2.5.0: [address classification](https://github.com/whitequark/ipaddr.js#readme),
  plus installed README/types, for public-unicast-only URL resolution.

HTTPS transport follows Node's [HTTPS request options](https://nodejs.org/api/https.html#httpsrequestoptions-callback)
and [TLS identity checks](https://nodejs.org/api/tls.html#tlsconnectoptions-callback):
connect to the vetted IP, send the original Host and TLS servername, keep
certificate verification enabled and disable agent pooling.

Installed with `bun add --exact --ignore-scripts`, respecting the project
three-day cooldown. No dependency lifecycle scripts were authorized or run;
`bun pm untrusted` lists the existing genai/protobufjs scripts as blocked.
Pi package versions remain unchanged. Bun's built-in HTMLRewriter was checked,
but a DOM parser makes heading/text extraction and entity decoding simpler.

## Boundaries and behavior

- Bind all local paths to the canonical workspace. Extract the writing pack's
  existing path/descriptor protections into a shared helper and retain its
  regression coverage. Reject escapes, symlinks, hard links and non-regular
  files for PDFs and note files, just as for drafts.
- Return source identity, stable numbered page/section anchors and text. Tool
  results enter the canonical session log before any following model request.
- Note recording requires a source/anchor previously extracted in the current
  process; re-extract after restart. Highlights must occur in the anchored text.
  Append Markdown containing source, anchor, quoted highlight and/or note.
- URL fetching accepts credential-free HTTPS on port 443 only. Resolve all DNS
  answers and reject any non-public address; pin a validated address for the
  connection. No redirects, proxy environment, cookies or real auth. Bound
  response bytes and request time; accept HTML only.
- Bound PDF bytes and pages and disable external resource loading. The bundled
  PDF.js parser does not execute PDF scripts.
  Scanned documents without text need OCR, which is deferred. PDF parsing runs
  in process; byte/page limits do not make it a hardened parser sandbox or
  impose a strict CPU deadline on malformed documents.
- Provide a built-in reading-only profile; user file profiles still override it.
  Default loads all shipped packs, now echo/writing/reading. Existing sessions
  require the original pack configuration when continuing.

The filesystem protections retain slice 4's limit: another process replacing
ancestor directories concurrently is outside this boundary. Notes do not
promise crash rollback or exactly-once append after arbitrary disk/process
failure. Fetched text is untrusted content, never harness instructions.

## Acceptance and handoff

Use a small committed, synthetic two-page PDF and a committed HTML article
fixture. Tests cover page/section extraction, entity decoding and excluded page
chrome/scripts, exact anchored-note bytes, invalid highlights/anchors,
filesystem escapes, URL private/mixed DNS answers and redirects, response caps,
errors and cancellation. Inject network dependencies for offline tests; no live
model or article request is acceptance evidence.

Criterion 12 is implemented for local PDFs and HTML articles with the permitted
EPUB deferral. Tools are `read_pdf`, `read_article` and `record_note`. The shared
workspace helper preserves the writing pack's existing boundary tests.

Verification:

- Focused reading extraction and network-policy fixtures: 35 passed after the
  final cancellation and note-encoding cases were added.
- `bun build src/packs/reading/extract.ts --target bun --outfile
  .tmp/slice5-reading/extract.js`, then extraction of the committed PDF through
  that bundle: passed, including page 2's expected anchor and text.
- Final `make validate`: passed TypeScript, Biome, all 94 offline tests
  (456 assertions), CLI bundling and the built executable usage/exit-code
  smoke check. The CLI journey checks parseable JSON events, canonical log
  equality before requests consuming PDF/article passages, exact note anchors,
  and replay after restart.
- `git diff --check`: passed.

Initial typechecks exposed differences from older PDF.js APIs (`loadingTask`
owns destruction; no `isEvalSupported` init parameter), DOM type availability,
and an overly broad transport-fixture return type. Installed declarations and
narrow consumed types resolved them without suppressions. The passing gates
above follow those corrections. Dependency installation added only the new
parser/address dependency trees; existing versions, including pi, are unchanged.

Verification is offline fixtures, transport mocks and self-review, not live
article fetching, real TLS connectivity, live inference or independent agent
review. Network-policy tests check the actual HTTPS option construction through
a restored mock, including pinned IP, original Host/servername, port 443,
certificate-verification defaults and disabled connection pooling. They do not
claim a live server journey. No real auth/config or personal content was read;
model calls and article responses were injected.

Changed paths: `src/packs/reading/` (implementation, tests, prompt and synthetic
fixtures), `src/workspace.ts`, `src/packs/writing/index.ts`, `src/packs/index.ts`,
`src/profile.ts`, `src/cli.test.ts`, `package.json`, `bun.lock`, `README.md`,
`AGENTS.md`, `docs/specs/ts-rework.md` and this record. Workstation root unchanged.
Branch `feat/reading-pack` is based on the full merged checkpoint above; the
owning PR records its delivered full commit. No implementation blocker remains;
owner PR review is pending.

Next after acceptance: slice 6's science pack, criterion 13. It needs an open
paper-search API fixture and an approval-enforced, workspace-confined Python
compute tool with a scrubbed environment and a timeout that kills the process.
Do not treat this slice's in-process PDF limits as that subprocess contract.
EPUB, OCR, remote PDFs and browser-rendered/authenticated pages remain deferred
or outside scope as listed above. No merge of this slice, installation, release
or deployment is included in this delivery.

