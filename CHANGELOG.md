# Changelog

One version number for every crate in the workspace. Pre-1.0, the second number is the breaking one.

## 0.4.0 - 2026-10-14

- **Row locks:** `FOR UPDATE | FOR SHARE [OF table] [NOWAIT | SKIP LOCKED]` are in the IR
  (`RowLockClause`). Leaves: `row_lock` (FOR UPDATE), `row_lock.share`, `row_lock.of`,
  `row_lock.nowait`, `row_lock.skip_locked`. Measured against the engines with a second session holding
  a row: NOWAIT raises and SKIP LOCKED skips on PostgreSQL, MySQL 8.4 and MariaDB 11.4. MySQL has all
  forms; MariaDB 11.4 has no `FOR SHARE` and no `FOR UPDATE OF` (syntax errors, probed) and refuses them;
  SQLite refuses every form (as before).
- **Fix:** the locking clause is printed after ORDER BY / LIMIT / OFFSET. It was printed before them,
  so `... ORDER BY x FOR UPDATE` translated to a syntax error on every target.
- Harness: a case can hold a lock in a second session (`Case::hold`) and expect an error on both
  engines (`Expect::BothError`).
- `unisql::analyse` names the new lock leaves; `docs/LEAF-CONTRACTS.tsv` has their contracts.

## 0.3.0 - 2026-10-14

- **MySQL/MariaDB `LIKE`/`ILIKE`:** the cause is the column collation, not a connection setting (that is
  SQLite). `like` now carries `text_collation`, so it is read from `config` like the other text leaves:
  strict `like` is `yes` under `collation=utf8mb4_bin`, with a known-bad control (default collation,
  must diverge). `ilike` carries `text_collation` and `ascii_only_case_folding` (verified: PostgreSQL's
  `'É' ILIKE 'é'` is false in the C locale, MySQL's `LOWER()` form is true). The leaf
  `like.like_case_sensitivity_is_connection_setting` no longer exists for the MySQL/MariaDB targets.

## 0.2.1 - 2026-10-14

- `ROADMAP.md` (unisql had none). The `Allowance` doc comment now says a variant's name and text are a
  contract: rewording is a new variant, not an edit (PersistAnt request). No behaviour change.

## 0.2.0 - 2026-10-14

- **Breaking (ids, matrix files, `analyse`):** constructs are now leaf ids. `cast` becomes
  `cast.to_<type>`; partial forms get their own leaf (`upsert.no_conflict_key`, `returning.update`,
  `trim.multi_char`, `fn:<agg>.order_by`, `distinct_on.star_projection`, `distinct_on.row_lock`,
  `distinct_on.set_operation`). `Untranslatable::subject()` returns the leaf; the `.refused` suffix
  is gone, a refusal is `enabled=no, reason=missing`. `analyse().constructs` names every form a
  statement needs, refused ones included (tested for every target).
- Matrix rows: a divergence a caller may accept is its own leaf `<construct>.<allowance>`; the strict
  leaf is `no/wrong`. Text collation is read from `config`: `yes` under the fixture collation, with a
  known-bad control (default MySQL/MariaDB `utf8mb4` collation, must diverge) recorded as `no/wrong`.
  `metadata` carries `suite` and `test`.
- `docs/LEAF-CONTRACTS.tsv` with a test that keeps it in step with the matrix files.

## 0.1.0 - 2026-10-14

- **Breaking (matrix files):** rows follow PersistAnt's merged row schema (DESIGN-KINDS.md section
  2.4 at 4fed258e): `how`/`evidence` are replaced by `config`, `enabled` (yes|no), `reason` and
  `metadata` (`tier=..;suite=..`). A refusal is `enabled=no, reason=missing`. Leaf ids and the MySQL
  default-collation known-bad run are not part of this change (still waiting on PersistAnt D2-D6).

## 0.0.3 - 2026-10-13

- `unisql::analyse(sql, source)`: target-free needs extraction (`constructs`, `refused`, `unreadable`),
  backed by a new IR walker in `unisql-core` (`needs::constructs`) and `Allowance::ALL`. Requested by
  the PersistAnt lane for deriving a program's needs from its statement catalogue.

## 0.0.2

- Lane `HANDOFF.md` is no longer tracked and is ignored (hygiene; no code change).

## 0.0.1

- First tracked state of UniSQL: IR, PostgreSQL/SQLite/MySQL/MariaDB readers and writers, parameter
  codec for array parameters, differential suites against real engines, matrix files, first CI.
