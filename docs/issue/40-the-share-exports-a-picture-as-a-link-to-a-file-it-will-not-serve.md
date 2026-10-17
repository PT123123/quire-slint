# 40 · the share exports a picture as a link to a file it refuses to serve, and answers 200 for every name it is asked

**Severity: medium** — `--scene image` makes the app store a real 146,748-byte PNG in
`<library>/attachments/` with a real `attachments` row, and `GET /api/page/102.md`
exports it as `![sample-gradient](quire://attachment/1)`. That address is not a route:
the read-only share has exactly three (`lan_server.rs:72-78`), and asking it for the
bytes answers `not found` four different ways. Meanwhile `/api/page/102.png`,
`/api/page/102.exe` and `/api/page/102.tar.gz` all answer **`200 OK` with that same
markdown**, because the id parse throws away everything after the first dot
(`:109-112`). Pull the frame back in and the picture becomes a paragraph whose text is
`!sample-gradient` — the bang is kept as a character, the link points at an id from
somebody else's library, and `select count(*) from attachments` in the destination is
**0**. Nothing in this document is a leak: no byte and no local path crosses the wire.
The defect is the opposite — the door writes a pointer it cannot honour, and then
reports success for every request that looks like honouring it.

## Evidence

`bash attprobe.sh` (legs A, B, C, D, E) and `bash attpath.sh` (the URL battery) —
`ftest/attprobe.sh`, `ftest/attpath.sh`, kept at `ftest/attprobe.log`, `ftest/attpath.log`
and `ftest/attprobe/` (`export/page-102.md`, `export/frame.md`, `snapA`, `snapC`, `snapD`).
One throwaway `--db` per case, every 5877 and 5899 arm gated on its own listener pid,
the mock peer stopped by the pid read off `netstat` and confirmed gone, libraries read
back from a `db + wal + shm` copy. No arm touched `%APPDATA%\Quire\quire.db`.

### A. The app builds three attachments, and the store behaves

Three sessions on one library, each running one of the app's own scenes
(`controller.rs:10552-10605`), each quitting gracefully:

```
--scene image      pid=171744  --quit rc=0 survived=no ended_lines=1
--scene file       pid=143108  --quit rc=0 survived=no ended_lines=2
--scene image-half pid=174436  --quit rc=0 survived=no ended_lines=3

1 sample-gradient  1.png    image/png         b=146748  640x400
2 quarterly-report.pdf 2.pdf application/pdf  b=1842000 0x0
3 sample-gradient  3.png    image/png         b=146748  640x400

blocks: image att=1 img_percent=100 · file att=2 img_percent=100 · image att=3 img_percent=50
kinds:  … image=2 toc=1 file=1 embed=1
folder: 146748 1.png   1842000 2.pdf   146748 3.png     files=3 rows=3
```

This is the control for the whole record, so it is worth saying what it rules out. The id
watermark works: three sessions, three ids, three files, no overwrite — `claim_attachment_id`
draws from `next_attachment_id`, which is seeded `max(id)+1` over the loaded rows at startup
(`state.rs:901-930`, `:3098-3102`). `2.pdf` is exactly the scene's fixed 1,842,000-byte
payload, `mime` came from the extension, and `thumb` is empty for both pictures because
640×400 is under `MAX_EDGE = 1280` (`attachment_store.rs:34,114`) — the downscale branch is
therefore **not** exercised here. `blocks.attachment` is set on all three rows, so the page
really does hold a picture and a file, and the file really is on disk.

### B. What the export door writes, and what the same server answers

The tail of `GET /api/page/102.md` (`HTTP/1.0 200 OK`, body 1681 B):

```
![sample-gradient](quire://attachment/1)

[quarterly-report.pdf](quire://attachment/2)

![sample-gradient](quire://attachment/3)
```

`export_service.rs:408-414` writes the image line and `:419-425` the file line, both from
`block.attachment`. The same three lines are in the frame — `/api/export` returned 7580 B
across 14 frames, with those links at lines 66, 68 and 70 — so this is not one route's
quirk; every Markdown-writing door writes the pointer and none writes the bytes.

Asking for the bytes:

```
GET /api/attachment/1   -> HTTP/1.0 404 Not Found   body=9 B ("not found")
GET /attachments/1.png  -> HTTP/1.0 404 Not Found   body=9 B
GET /1.png              -> HTTP/1.0 404 Not Found   body=9 B
GET /api/file/1         -> HTTP/1.0 404 Not Found   body=9 B
GET /api/export.png     -> HTTP/1.0 404 Not Found   body=9 B
```

There is no attachment route to miss: the `match path` at `lan_server.rs:72` has arms for
`/` + `/api`, `/api/pages`, `/api/page/…` and `/api/export`, and the index page it serves at
`:76` advertises exactly those three.

### C. The same door answers 200 for any file name (attpath.sh)

Page 102 under ten spellings, each row printed with the body's md5:

```
/api/page/102.md        200 | first=[# Getting Started]  md5=5fcd74e97a
/api/page/102           200 | …                          md5=5fcd74e97a
/api/page/102.png       200 | …                          md5=5fcd74e97a
/api/page/102.jpg       200 | …                          md5=5fcd74e97a
/api/page/102.pdf       200 | …                          md5=5fcd74e97a
/api/page/102.tar.gz    200 | …                          md5=5fcd74e97a
/api/page/102.exe       200 | …                          md5=5fcd74e97a
/api/page/102.PNG       200 | …                          md5=5fcd74e97a
/api/page/102.bin       200 | …                          md5=5fcd74e97a
/api/page/102.md.md     200 | …                          md5=5fcd74e97a
```

`md5=5fcd74e97a` is the md5 of `attprobe/export/page-102.md` — one page, ten URLs. The raw
answer to `/api/page/102.png` is

```
HTTP/1.0 200 OK
Content-Type: text/plain; charset=utf-8
Content-Length: 1681
Connection: close
```

and the guard rails that `28` measured are all still up in the same run: `/api/page/999.md`,
`/api/page/0.md`, `/api/page/-1.md` → `no such page`; `/api/page/102abc.md`, `/api/page/.md`
→ `bad page id`; template id 114 → `no such page`, not its markdown. `+102`, `0102` and
`102.0` reach page 102 too, which is `i64` parsing, not a bug worth a separate line. What
miles is that a *client* cannot tell a served file from a served page: it gets `200`,
`text/plain` and 1681 bytes for a request whose last element was `.png`.

### D. What the reader keeps after a round trip

`--pull` of the captured frame (`pullserver.js` on `127.0.0.1:5899`, pid 165288 gated and
then killed by that pid) into a fresh library; console says `imported 14 pages`:

```
kinds:            bullet=93 paragraph=64 … embed=1     (image=0 file=0)
attachment rows:  0
image/file blocks:0
rows whose text contains "quire://":                    []      ← the pointer is not in the text
```

The last line looks like the link vanished. It did not — it is in `marks`, and joining
`marks` to `blocks` (`snapC`) gives the three rows the export wrote:

```
block        kind       start end mark  url                     text
1000000181   paragraph  1     16  link  quire://attachment/1    !sample-gradient
1000000182   paragraph  0     20  link  quire://attachment/2     quarterly-report.pdf
1000000183   paragraph  1     16  link  quire://attachment/3    !sample-gradient
```

`--open` of the page `.md` into another fresh library (`snapD`) produces the identical three
rows on page 119 with ids 1000000170-172 and `attachments` = 0. So both doors keep the name,
keep the pointer as a hyperlink, and drop the file, the row, the width and the MIME type. The
`start=1` on the picture rows is the second half of the finding: the importer's `[` branch
(`import_service.rs:469-488`) starts at the bracket, so the `!` of `![…]` is emitted as an
ordinary character and lands in the sentence. On the reader's screen that line reads
**!sample-gradient** as a hyperlink.

### E. Where the pointer can go

Nothing. The app's link handler has two `quire://` arms — `quire://block/`
(`controller.rs:5460`) and `quire://page/` (`:5466`) — and its comment at `:5459` says unknown
`quire` URLs are "swallowed, not shelled out"; `crate::core::embed::is_openable`
(`core/embed.rs:111-114`, which admits only `http://`, `https://` and `mailto:`) gates the shell
call at `:5479`. Grepping `quire://` across `src/` finds those two handlers, the `page_of`
parser's `PAGE_SCHEME` (`core/reference.rs:23`), the places that *build* page/block addresses
(`state.rs:2697`, `:2700`, `:13094`) and a test string (`platform/mod.rs:682`) — no arm anywhere
matches `quire://attachment`, and the only writer of that form is `export_service.rs:411,422`. A
picture exported through the share therefore arrives as a dead link with a stray exclamation
mark.

## Cause

1. **The scheme is invented by the writer and implemented by nobody.** `render` writes
   `quire://attachment/<id>` for `Image` (`export_service.rs:411`) and `File` (`:422`). The
   `<id>` is a row id in *this* library — `attachments.id` is an `INTEGER PRIMARY KEY` handed
   out by a per-session counter (`migrations.rs:189-198`, `state.rs:3098-3102`) — so the
   number means nothing to the reader, and the same server that wrote it has no route for it
   (B). The doc comment above the image arm, `:405-407`, is honest about the intent — "Re-import
   keeps the name as a link, not a picture" — but it describes only half of what the reader
   needs, and it never asks whether the link resolves.
2. **`export_page` is a text-only door and the media layer is not text.** This is the same wall
   `38` hit from the other side: the LAN doors get `&|_| None` for databases (`:37-39`), and
   attachments do not even need a reader — they need *bytes*, which the frame format
   (`lan_client.rs`, `unframe_workspace`, `lan_server.rs:196-224`) has no place for.
3. **The bang is a syntax the importer does not own.** `!` has no branch in `parse_inline`, so
   GFM image syntax degrades one character worse than the link syntax the exporter's own comment
   relies on.
4. **The page route's id parse is a `split('.')`.** `strip_prefix("/api/page/").and_then(|s|
   s.split('.').next()).and_then(|s| s.parse::<i64>())` (`lan_server.rs:109-112`) was written so
   `102.md` works; it also makes `102.<anything>` work, which turns "that file does not exist"
   into `200 OK` with someone else's content.

## What this is not

- **Not a disclosure.** The share serves no bytes and no local path: `Content-Type` is
  `text/plain` for every response (`:235`), the row's `file`/`thumb` never enter the markdown,
  and the markdown carries only the *label*. `28`'s traversal control still holds — `..%2f`
  forms hit the `i64` parse and answer `bad page id`.
- **Not `38`.** The database block writes *nothing* through these doors; the attachment block
  writes a name and a pointer. Different loss, different function, and no shared cause —
  `render`'s image arm never consults the `database` reader.
- **Not the sync door.** Sync is the one path that does carry media: the engine asks peers for
  attachment bytes (`engine.rs:100-114,336-338`), `state.rs:15938-15950` runs the received
  buffer through the store's own `import_bytes`, and `sync_stored_attachment` (`:15203-15224`)
  deliberately rebuilds `file` as `{id}.{ext}` on this device because — in its author's words —
  "the peer's own `file` and `thumb` are network input, and joining them … writes at whatever
  path they hold". The Markdown doors are missing the *bytes*, not a sanitisation.
- **Not a broken storage layer.** A's three rows and three files, `blocks.attachment`,
  `mime`, `bytes`, `width`/`height` and `img_percent` all persist across graceful quits.
- **Not rendering.** No pixel claim here: everything is `node:sqlite` on a copied library or
  raw HTTP/1.0 responses. Whether `!sample-gradient` as a hyperlink is *legible* on screen was
  not photographed.

## Candidate fixes

1. **Serve the bytes, or stop advertising them.** If these doors are meant to be readable by
   other tools, `/api/attachment/<id>` should exist on the same host, return the stored file
   with a real `Content-Type`, and respect the template and read-only rules — and the exporter
   should then write a URL that resolves *for the reader*, i.e. an `http://host:port/…` form,
   not a private scheme. Cost: the credential-free share (`28`) becomes a file server, so this
   fix should not land before `21`/`28`'s authentication question is settled; a LAN-wide
   `GET /api/attachment/<n>` walks the library folder one id at a time.
2. **The cheaper fix, and the one consistent with `38`: write a visible refusal.** A picture
   that cannot travel should say so — `（图片 sample-gradient，1.4 MiB，未随共享导出）` — instead
   of emitting a hyperlink that resolves to nothing. Same for the file row.
3. **Either way, drop the bang.** Give `parse_inline` a `!` arm that consumes `![label](url)`
   into a link mark with no leading character (`import_service.rs:469`), so the degraded form
   reads as the label and not as a typo.
4. **Tighten the page parse**: `strip_suffix(".md")` (or require exactly that) and then parse the
   remainder whole, so `102.png` answers `400`. Keep `/api/page/…` for pages only, and give any
   future media route its own prefix.
5. **A gate that fails today**: seed `--scene image` + `--scene file`, then assert (a) every
   `/api/page/102.<ext>` for `ext` in `png|jpg|exe|tar.gz` answers 4xx, (b) a page round trip
   through `--open` contains no `!`-prefixed paragraph and no `marks.url` starting with
   `quire://attachment/`, and (c) if a media route is ever added, its body md5 equals the file in
   the source `attachments/` folder.

## Not measured

- **The click.** D/E establish that no code resolves `quire://attachment/…`, but the inert link
  was never activated — that needs a pointer in the editor, and this pass does not drive desktop
  UI. `is_openable` was read (`core/embed.rs:111-114`), not exercised.
- **An id that collides.** The destination libraries held zero attachment rows, so an imported
  `quire://attachment/1` was never tested against a *local* attachment 1. With no handler
  (E) the collision should be inert, but that is an inference.
- **The downscale path.** Both fixtures are 640×400, under `MAX_EDGE`, so no `.cache.png` was
  written and `thumb` stayed empty in every row (A). A picture over 1280 px, and the GIF
  first-frame rule at `attachment_store.rs:333-334`, were not reached.
- **Page covers** (`controller.rs:11005`, `pick_cover` at `:8361-8374`) and the clipboard
  attachment paste (`state.rs:3139`) were read, not driven. Whether a cover leaks into
  `/api/export` is untested — the frame in B came from a page with a picture *block*.
- **`![name]()`** — the no-row form `export_service.rs:412` writes when `block.attachment` is
  `None` — was never produced; seeding that state means writing `blocks.attachment` by hand.
- **A second machine**, as in `37`: every request came from `127.0.0.1` on the box that answered.

## How the harness got it wrong first

- `select marks from blocks` answered `Error: no such column: marks`. Marks are a **table**
  (`migrations.rs:142-149`), and the first read of leg C therefore reported "rows holding the
  pointer: []" — which is what a lost line looks like from the wrong side of the join. The
  link survived all along; the corrected query is in D.
- Ten URLs returning "200" would not have distinguished the page from an error page of the same
  length, so `ask` prints the body's md5 next to the status. `5fcd74e97a` is what makes C one
  fact rather than ten guesses.
- `--scene image` and `--scene image-half` produce **byte-identical** fixtures (both 146,748 B),
  so "1.png and 3.png are the same size" looks like an overwrite until the row count and the two
  distinct file names are checked. The control in A exists to close that gap before B needs it.
- The mock peer is stopped by the pid `netstat` reported for `127.0.0.1:5899`
  (`taskkill //PID 165288 //F`) and its port re-checked empty, not by `$!` — the same lesson
  `34`/`36` paid for.
