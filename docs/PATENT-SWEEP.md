# Patent sweep: UniSQL 0.27.x @ 7692be8

Date: 2026-10-25; Run by: Claude Sonnet 5.5 for the maintainers; Method: AI-assisted web search of Google Patents and read of the closest
hits through a summarising page-fetch tool; **NOT legal advice; NOT a freedom-to-operate opinion.**
Covers: the workspace at version line **0.27.x** (swept at commit `7692be8`, 0.27.7). Re-sweep when a new family of capability is added, and at least yearly.

## Ideas swept

What UniSQL does that might be new or unusual (mechanisms, not features):

1. A method that translates the text of one SQL statement from a source dialect to a target dialect by reading it into a dialect-neutral intermediate
   representation and printing it with a per-target writer, and that returns **no text at all** (an error naming the construct) when the target cannot give the
   construct the source's meaning.
2. A translation carries a per-construct report classifying how the target gave each construct: exactly, by emulation, or only under a named assumption.
3. A construct that is the same only under an assumption translates only when the caller names that assumption (an "allowance"); without it the translation is refused.
4. Per-leaf matrix files record, for each construct, target backend version and configuration, whether it is enabled, and the test that shows it, generated from
   differential runs; each leaf has a one-sentence contract.
5. A differential harness runs the source statement on a real source engine and the translation on a real target engine over the same fixture data and compares the
   result sets strictly; a statement a target refuses is a recorded outcome, not a skipped case.
6. A translation returns the order in which the source's numbered parameters appear in the target's text (so positional `?` targets bind correctly when a
   parameter is used twice or reordered) and a per-parameter binding kind (for example an array parameter becomes JSON text on targets without arrays).
7. A procedural macro translates a literal SQL statement for several named targets at build time and embeds the results as constant data; placeholders must be
   contiguous, and a refused or unassumed construct is a compile error.
8. Optional column-class facts (from DDL or a snapshot) change how the translation treats comparisons (text versus non-text columns), and a pluggable checker
   receives the parsed statement and the declared parameter types before translation.
9. Emulations of constructs a target lacks: `DISTINCT ON` by a window function, `NULLS FIRST/LAST` by a leading `IS NULL` sort key, an upsert `WHERE` by `IF()` in
   each assignment, `date_trunc`/`extract` by `strftime` on text timestamps.
10. A target-free analysis of a statement lists the construct identifiers it needs and which of them some target refuses.
11. A prepare check: translate a catalogue of statements for each target and `PREPARE` each result on a real engine without executing it.
12. Text-function mappings accepted only for argument forms on which the two engines were observed to agree (for example `lpad` with a literal length and a
    non-empty literal pad), and refused otherwise.

## Queries (source, query, date)

All on Google Patents through a web-search tool (US-only results), 2026-10-25. The tool returns titles and snippets; it is not a full-text boolean search.

| # | Idea | Query |
|---|---|---|
| Q1 | 1 | `patent translating SQL statement between database dialects intermediate representation refuse unsupported construct` |
| Q2 | 1 | `patent SQL query translation from first database dialect to second dialect abstract syntax tree rewrite rules` |
| Q3 | 5 | `patent differential testing compare results of query on source database and translated query on target database equivalence` |
| Q4 | 7 | `patent compile-time macro translate SQL statement for multiple database targets embedded constants placeholder check` |
| Q5 | 2, 3 | `patent translated query fidelity report classification exact emulated conditional database dialect conversion warnings semantic differences` |
| Q6 | 9 | `patent emulating DISTINCT ON or NULLS FIRST using window function row_number rewrite query for database lacking feature` |
| Q7 | 6 | `patent converting positional and named bind parameters between database drivers parameter order mapping translated SQL` |
| Q8 | 8 | `patent database schema aware SQL translation column types change comparison semantics collation conversion between database systems` |
| Q9 | 9 | `patent upsert merge statement conversion between database systems ON CONFLICT ON DUPLICATE KEY emulation translating insert-or-update` |

Ideas 4, 10, 11 and 12 had no query of their own: 4 and 10 are about this project's test and reporting artefacts, 11 is covered by Q3/Q4 results, 12 is a
data table of observed engine behaviour. **A later pass should add queries for them.**

## Hits

The independent claims below were read **as summaries produced by a page-fetch tool** (it would not reproduce claim text, so quoted fragments are short and some
steps are paraphrased). Status is Google Patents' listing and is an assumption, not a legal conclusion. "Overlap" is against the closest idea.

| # | patent | title | assignee | status | claim(s) read | overlap | our difference | verdict |
|---|---|---|---|---|---|---|---|---|
| 1 | US9582500B2 (pub. US20140280259A1) | Run-time SQL language translation | Oracle International Corp | granted 2017-02-28; listed active, adjusted expiry 2034-12-19 | independent 1 (method), 9 (system), 17 (medium), summarised: intercept a query containing dynamically generated SQL in a foreign dialect "incompatible" with the native database, at run time; exclude literals, match the rest against canonical templates, translate with the templates, reinsert the literals, before returning results | idea 1 (translate SQL between dialects), partly 6 | UniSQL is a library call or build-time macro on a given statement; it does not intercept an application's queries on a database session, has no canonical-template lookup and no literal removal/reinsertion, and refuses rather than falling back to a look-up table | clear on the independent claims as summarised (provisional: claim text not read verbatim) |
| 2 | US10216826B2 (pub. US20160063063A1) | Database query system | Salesforce Inc | granted 2019-02-26; listed active, adjusted expiry 2037-10-08 | independent 1 (system), 6 (method), 14 (medium), summarised: a statement mapping database of first/second-database statements; receive a user query containing a second-database statement; parse it, transform to a tree, generate (unparse) the first-database statement; display portions in real time with each keystroke; execute in the first database | idea 1 (parse to tree, unparse to a target) | UniSQL has no statement-mapping database, no keystroke-time display and no execution (it returns text); its refusal and report steps are not recited | clear on the independent claims as summarised (provisional) |
| 3 | US7664795B2 (pub. US20050149537A1) | Apparatus and method for database migration | Microsoft Corp | granted 2010-02-16; listed **expired** (adjusted expiry 2025-03-02) | independent 1, 9, 13, summarised: migrate a database (schema, stored-procedure SQL to trees to target dialect) and install a converter wrapper on the target driver; middleware with a cache of converted statements | idea 1 | expired; UniSQL also migrates no schema and has no driver wrapper or statement cache | clear (expired) |
| 4 | US8108415B2 (pub. US20090063441A1) | Query transformation | IBM, later SAP SE | granted 2012-01-31; listed expired (fee related) | independent 1, 11, 16, summarised: rewrite a query with control breaks (OVER (PARTITION BY) aggregates) into a derived table with GROUP BY so a target can compute it | idea 9 (rewriting a construct the target lacks) | different construct (control-break aggregates); UniSQL's emulations (DISTINCT ON, NULLS ordering, upsert WHERE, strftime) are not recited | clear (expired; different construct) |
| 5 | US6535874B2 (pub. US20010013038A1) | Technique for providing a universal query for multiple different databases | IBM | granted 2003-03-18; listed expired (adjusted expiry 2017-11-01) | independent 1, 7, summarised: one query sent to servers for several databases, each server builds a database-specific query | idea 1 | expired; federated query fan-out, not a dialect translator | clear (expired) |
| 6 | US8645346B2 (pub. US20120323956A1) | Composable SQL query generation | Microsoft Corp | granted 2014-02-04; listed expired (lapsed for fees, 2026-02-04) | independent 1, 11, 14, summarised: operator graph from a data model to composable SQL, remotable to a target source, streamed results; dependent claims 13 and 17 add post-processing of operations the source lacks | idea 9, 2 (refusing vs post-processing) | expired; UniSQL builds no operator graph from a data model, executes nothing and does no post-processing | clear (expired) |

Seen and set aside without reading claims (different subject): US20190243831A1 (natural-language query translation), EP1759315B1 / US20050289125A1
(XQuery to SQL AST conversion), US8463801B2, US7120645B2 (XML/XQuery rewriting), US7912834B2 / US8103689B2 (materialized-view rewrite of rank and row_number
queries), US5257366A (bind-file bridge for heterogeneous databases), the Salesforce multi-tenant SQL family (US10649987 and others; parameters numbered in lexical order,
a wrapped JDBC connection translating a proprietary dialect), US6877000B2 (convert SQL to ODBC), US7912835B2 (compile-time opcodes for bind variables), US8145655B2.
The multi-tenant family and US6877000B2 are the likeliest to deserve a claim-level read in a follow-up, for ideas 6 and 7.

## Needs review (to the maintainers)

None from this pass. Hits 1 and 2 are **active** patents whose independent claims share the broad theme of "translate SQL between dialects"; the differences above are
real on the summaries I could read, but a full-text read of their independent claims by someone who can reproduce the claim language is the one step that would
move them from "clear (provisional)" to "clear".

## Limitations

- AI-assisted search and read, by a model; not legal advice and not a freedom-to-operate opinion. A lawyer's review is a separate purchase.
- Nine queries on one source (Google Patents through a snippet-returning search tool, US-only results). No USPTO full-text, EPO/Espacenet or WIPO queries; no
  non-English patents beyond what Google returned; no classification-code search (for example CPC G06F 16/24 and 16/25).
- Claims were read through a summarising fetch tool, not as verbatim text, so verdicts on the two active patents (hits 1 and 2) are provisional. For hits 1 to 4 the
  page text was longer than the tool read (about the last 5 to 20 per cent was not read); for hit 6 the claims section was read in a second call.
- Ideas 4, 10, 11 and 12 were not searched separately (see above). Open-source and academic prior art (SQLGlot and the like) is covered by `docs/DESIGN.md`
  section 4 and `docs/LICENCES.md`, not here.
- The search found nothing that claims a refusal-by-default translator with a per-construct fidelity report or named allowances; absence from these nine
  queries is not proof that none exists.
