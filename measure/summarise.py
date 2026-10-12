"""Summarise classifier output: counts by corpus group and tier, reasons grouped by tier."""
import collections
import json
import re
import sys


def group(r):
    f = r['file']
    if r['corpus'] == 'humboldt':
        if f.startswith('backend/migrations/') or f.startswith('docs/'):
            return 'humboldt:migrations(.sql)'
        if f.startswith('backend/tests/'):
            return 'humboldt:tests'
        return 'humboldt:src'
    if r['corpus'] == 'auth-framework':
        if f.endswith('.sql'):
            return 'auth:migrations(.sql)'
        if f == 'src/storage/postgres.rs':
            return 'auth:postgres.rs'
        if f in ('src/storage/sqlite.rs', 'auth-storage-sqlite/src/lib.rs'):
            return 'auth:sqlite(' + f + ')'
        return 'auth:other(' + f + ')'
    if r['corpus'] == 'lightbulb':
        return 'lightbulb:sql' if r['kind'] == 'sqlfile' else 'lightbulb:rust'
    return r['corpus']


SQLISH = re.compile(r'\$\d|\bfrom\b|\binto\b|\bset\s+\w+\s*=|\btable\b|\btrigger\b|\bdatabase\b', re.I)
DML = {'Query', 'Insert', 'Update', 'Delete', 'Merge'}


def is_prose(r):
    return (not r['parsed']) and r['kind'] != 'sqlfile' and not SQLISH.search(r['sql'])


def cat(r):
    ks = set(r.get('kinds', []))
    if ks and ks <= DML:
        return 'dml'
    if ks & {'CreateTable', 'CreateIndex', 'AlterTable', 'Drop', 'CreateView', 'CreateSchema'} and not ks & {'CreateTrigger', 'CreateFunction', 'DoBlock'}:
        return 'ddl'
    return 'other' if ks else 'unparsed'


def main():
    rows = [json.loads(l) for p in sys.argv[1:] for l in open(p)]
    prose = collections.Counter(group(r) for r in rows if is_prose(r))
    print('excluded as prose (parse failed, no SQL-shaped token):', dict(prose), 'total', sum(prose.values()))
    rows = [r for r in rows if not is_prose(r)]
    kinds_ct = collections.defaultdict(collections.Counter)
    for r in rows:
        t = r['tier'] if r['parsed'] else 'unparsed'
        kinds_ct[group(r)][cat(r) + ':' + t] += 1
    for g in sorted(kinds_ct):
        print(f'  by-category {g:46s}', dict(sorted(kinds_ct[g].items())))
    print()
    by = collections.defaultdict(collections.Counter)
    reasons = collections.defaultdict(collections.Counter)
    errs = collections.defaultdict(collections.Counter)
    for r in rows:
        g = group(r)
        t = r['tier'] if r['parsed'] else 'unparsed'
        by[g][t] += 1
        by[g]['total'] += 1
        if r.get('templated'):
            by[g]['(templated)'] += 1
        for x in r.get('reasons', []):
            tier, why = x.split(':', 1)
            reasons[g][(tier, why)] += 1
        if not r['parsed']:
            errs[g][r['error'][:70]] += 1
    order = ['total', 'exact', 'emulated', 'conditional', 'refused', 'unparsed', '(templated)']
    for g in sorted(by):
        c = by[g]
        print(f'{g:50s} ' + ' '.join(f'{k}={c[k]}' for k in order))
    print()
    if '--detail' in sys.argv[0:0] or True:
        for g in sorted(reasons):
            print('##', g)
            for (tier, why), n in sorted(reasons[g].items(), key=lambda kv: (-kv[1], kv[0])):
                print(f'   {n:4d} {tier:11s} {why}')
    print()
    for g in sorted(errs):
        print('## unparsed', g)
        for e, n in errs[g].most_common(8):
            print(f'   {n:4d} {e}')


main()
