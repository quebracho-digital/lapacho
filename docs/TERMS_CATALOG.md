# Terms catalogue — design

How the terms analyser stores what it knows, so it can grow by **country**,
by **company** and by **feature** without a rewrite. The plan and its order
are in `ROADMAP.md` (🧩 Plugins → *Read the terms before accepting them*).
The evaluation set lives in a separate repository, `terms-eval`.

Status: design, 2026-10-06. Nothing here is built.

## The rule that keeps it growing evenly

**Countries, companies and taxonomy are data; only new kinds of feature are
code.** Adding Mexico, adding Mercado Libre, or adding a category must not
touch the app. Adding "compare two services" may.

## The model

```
Group ─┬─ Company (legal entity, country of registration)
       │     └─ Service (what users know: "WhatsApp")
       │           ├─ domains, app ids          (how a page or an app is matched)
       │           └─ Document (kind × jurisdiction × language)
       │                 └─ Version (text hash, fetched_at, source)
       │                       └─ Analysis (taxonomy version, analyser,
       │                                    labels: category → quotes + severity)
Jurisdiction (country)
       ├─ legal map: category → statutes, "may be void here" rules
       ├─ actions: templates (ARCO request, cancellation, withdrawal)
       └─ authorities (where to complain)
Taxonomy (versioned)
       └─ categories, boundary rules, severity rubric
```

- **Group → Company → Service.** Meta owns WhatsApp and Instagram through
  more than one legal entity, and which entity signs the contract is itself
  information (*"your data controller is in Ireland"*). A service belongs to
  one company; a company to at most one group.
- **Document = kind × jurisdiction × language.** Kinds: `terms`, `privacy`,
  `cookies`, `eula`, `pricing`, more later. Spotify's terms for Argentina
  and for the US are two documents (one has arbitration, the other doesn't).
  That difference, side by side, is a feature.
- **Version** is identified by the SHA-256 of the normalized text and
  records *how* it was obtained: `fetch` (plain HTTP), `render` (headless
  browser), `archive` (Internet Archive), `manual` (a person saved it). A
  version is never edited: a change is a new version, and the diff between
  two is computed, not stored.
- **Analysis** belongs to one version and names the taxonomy version and the
  analyser (model + prompt version) that produced it. Labels are the same
  shape as `terms-eval`: category → exact quotes + severity. A human review
  is a field, not a separate table: `reviewed_by`, `reviewed_at`.
- **Jurisdiction** carries everything country-specific: which law makes a
  category questionable there (Argentina: Ley 24.240 art. 37, Código Civil
  y Comercial arts. 988 and 1119, Ley 25.326; Spain: Directive 93/13/EEC,
  GDPR), the action templates, and where to complain (AAIP, Defensa del
  Consumidor). A new country is a new jurisdiction record plus translations,
  not code.
- **Taxonomy** is versioned (`major.minor`). Category ids are stable and
  never reused; a minor version adds categories, a major one changes a
  definition. An old analysis stays valid under the version it names.

IDs are readable slugs, stable forever: `meta`, `whatsapp`,
`whatsapp/terms/ar/es`, and a version is its hash.

## What reaches the device

Packs, the way dictionaries already work (`docs/DICTIONARIES.md`):

- One pack per **jurisdiction × language**:
  `lapacho-terms-ar-es-<date>.pack`. It is a SQLite file holding, for
  each document, only its **current** analysis, the summary of what
  changed since the previous version, the legal map and the actions. The
  history stays in the public dataset.
- **Size budget:** ~5–10 KB per analysed document. 500 documents for a
  country is ~5 MB, about a dictionary. That budget is checked in CI.
- **Signed, not hash-listed.** Dictionaries are official by a hash compiled
  into the APK, which works for files that change once a year. A catalogue
  changes monthly, so packs carry an Ed25519 signature checked against a
  public key in the app. The private key never touches CI that publishes
  APKs.
- Downloaded **whole**, never queried per service, so nobody learns which
  service you looked up (see the roadmap's metadata layers).

On the device, two stores that never mix:

| Store | What | Written by |
|---|---|---|
| catalogue pack | public analyses, read-only, replaced on update | the pack |
| *my services* | which services you analysed or accepted, and the version hash you accepted | you, encrypted like the clipboard history |

Change alerts are a local join of the two: when a new pack arrives, any
service in *my services* whose current version hash differs from the one you
accepted gets an alert with the diff summary. Nothing leaves the device.

## How features plug in

Lapacho's plugins today are built-in (search and replace) or external
commands (desktop only). The terms features are built-in plugins that read
the same packs. Each one declares the document **kinds** and
**jurisdictions** it handles, and gets the data from there:

| Plugin | Needs | Country-specific through |
|---|---|---|
| analyse (local model or share sheet) | the taxonomy | the legal map, applied to its output |
| lookup | catalogue pack | the pack's jurisdiction |
| changes since you accepted | pack + *my services* | — |
| actions (ARCO, cancel, withdraw) | jurisdiction actions | templates per country |
| compare by country | two packs | — |

A plugin that needs a country that has no data says so; it never falls back
to another country's law.

## The pipeline that builds the catalogue (SER5)

Stages, each idempotent and keyed by document, so one failure doesn't stop
the rest:

1. **Discover**: the list of services per country (top N by use), their
   domains and their legal pages. Kept by hand at first, in the repo.
2. **Fetch** with the lightest source that works: `fetch` → `render`
   (Playwright, headless Chromium, honest user agent, robots.txt respected,
   one request per site at a time) → `archive` → `manual`. Mercado Libre
   and Mercado Pago block automated browsers (HTTP 403, tested 2026-10-06):
   they come in as `manual` or `archive`, not by evading the block.
3. **Normalize and hash**: same text, same hash; a new hash is a new version.
4. **Diff** against the previous version: which clauses appeared, went or
   changed severity.
5. **Analyse** the new version with the current analyser.
6. **Review**: every high-severity label waits for a person before it's
   published. The rest publish as `silver` and say so.
7. **Publish**: build and sign the packs, and push the history to the public
   dataset.

## Growth order

1. **Argentina**, Spanish: the top 50 services. Legal map and actions first.
2. **Spain**: same language, EU law; most global services have a separate
   EU version.
3. **Mexico, Chile, Colombia, Uruguay**: Spanish, one legal map each.
4. **Brazil**: Portuguese and LGPD; the analyser and taxonomy are tested in
   a second language here.
5. Anything else when someone maintains its legal map. A country without a
   maintainer is not added.

## Not decided

- Where the pack signing key lives, and who can sign.
- Whether the public dataset carries quotes. That waits for a legal review
  of quoting someone else's terms.
- SQLite or a flatter format for packs. SQLite is what the app already
  ships.
