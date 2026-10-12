# SPDX-License-Identifier: MIT OR Apache-2.0
"""Final counts: DML only and all statements, with 'unmapped' split from refused-by-design."""
import collections
import json
import sys

sys.argv = ['x']
exec(open('summarise.py').read().replace('\nmain()\n', '\n'))


def final_tier(r):
    if not r['parsed']:
        return 'unparsed'
    t = r['tier']
    if t != 'refused':
        return t
    by_design = [x for x in r['reasons'] if x.startswith('refused:') and '(no mapping)' not in x]
    return 'refused' if by_design else 'unmapped'


rows = [json.loads(l) for c in ('auth', 'lightbulb', 'humboldt') for l in open(c + '.out.jsonl')]
rows = [r for r in rows if not is_prose(r)]
ORDER = ['exact', 'emulated', 'conditional', 'unmapped', 'refused', 'unparsed']
for label, pred in (('DML', lambda r: cat(r) == 'dml'), ('DDL', lambda r: cat(r) == 'ddl'),
                    ('OTHER/UNPARSED', lambda r: cat(r) in ('other', 'unparsed'))):
    print('##', label)
    tab = collections.defaultdict(collections.Counter)
    for r in rows:
        if pred(r):
            tab[group(r)][final_tier(r)] += 1
    for g in sorted(tab):
        c = tab[g]
        print(f'  {g:50s} n={sum(c.values()):4d} ' + ' '.join(f'{k}={c[k]}' for k in ORDER))

print('\n## unmapped function names (rule-table gaps), DML+DDL')
un = collections.Counter()
for r in rows:
    if final_tier(r) == 'unmapped':
        for x in r['reasons']:
            if '(no mapping)' in x:
                un[x.split(':', 2)[2]] += 1
for k, v in un.most_common():
    print(f'  {v:3d} {k}')

print('\n## refused-by-design reasons, DML statements only, distinct statements')
rd = collections.Counter()
for r in rows:
    if cat(r) == 'dml' and final_tier(r) == 'refused':
        for x in set(x for x in r['reasons'] if x.startswith('refused:') and '(no mapping)' not in x):
            rd[x.split(':', 1)[1]] += 1
for k, v in rd.most_common():
    print(f'  {v:3d} {k}')
