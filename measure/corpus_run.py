"""Feed the DML statements of the three corpora to the real translator and summarise.

usage: python3 -I corpus_run.py <corpus-binary> [label]
Selection rule (same as final.py): not prose, classify said every statement kind is DML.
Templated statements get `{..}` holes replaced by identifiers exactly as the classifier did.
"""
import collections
import json
import re
import subprocess
import sys

sys.argv_saved = sys.argv
exe = sys.argv[1]
label = sys.argv[2] if len(sys.argv) > 2 else 'run'
sys.argv = ['x']
exec(open('summarise.py').read().replace('\nmain()\n', '\n'))
HOLE = re.compile(r'\{[A-Za-z0-9_:.?]*\}')

src = {}
for c in ('auth', 'lightbulb', 'humboldt'):
    for l in open(c + '.jsonl'):
        r = json.loads(l)
        src[(r['corpus'], r['file'], r['line'])] = r

jobs = []
for c in ('auth', 'lightbulb', 'humboldt'):
    for l in open(c + '.out.jsonl'):
        r = json.loads(l)
        if is_prose(r) or cat(r) != 'dml':
            continue
        if r.get('source_dialect') != 'postgres':
            continue  # the SQLite-written statements are the oracle, not the source
        raw = src[(r['corpus'], r['file'], r['line'])]['sql']
        if r.get('templated'):
            n = [0]

            def rep(m):
                n[0] += 1
                return f'tpl_{n[0]}'
            raw = HOLE.sub(rep, raw)
        jobs.append({'id': len(jobs), 'group': group(r), 'sha': r['sha'], 'file': r['file'], 'line': r['line'], 'sql': raw})

target = sys.argv_saved[3] if len(sys.argv_saved) > 3 else 'sqlite'
p = subprocess.run([exe, target], input='\n'.join(json.dumps(j) for j in jobs) + '\n', capture_output=True, text=True)
if p.returncode != 0:
    print(p.stderr[-2000:])
    raise SystemExit(1)
res = [json.loads(l) for l in p.stdout.splitlines()]
assert len(res) == len(jobs), (len(res), len(jobs))
json.dump(res, open(f'corpus-{label}.json', 'w'))

by = collections.defaultdict(collections.Counter)
det = collections.defaultdict(collections.Counter)
al = collections.defaultdict(collections.Counter)
for r in res:
    g = r['group']
    by[g][r['status']] += 1
    by[g]['n'] += 1
    if r['status'] in ('refused', 'not_in_ir', 'syntax'):
        det[g][(r['status'], r['detail'])] += 1
    for a in r['allowances']:
        al[g][a] += 1
shas = sorted({(j['group'].split(':')[0], j['sha'][:8]) for j in jobs})
print(f'## {label}: PostgreSQL -> {target}, real translator, all allowances granted; SHAs {shas}')
for g in sorted(by):
    c = by[g]
    print(f"  {g:34s} n={c['n']:4d} translated={c['translated']:4d} conditional={c['conditional']:4d} "
          f"refused={c['refused']:3d} not_in_ir={c['not_in_ir']:3d} syntax={c['syntax']:3d} error={c['error']}")
print()
for g in sorted(det):
    print('##', g)
    for (st, d), n in sorted(det[g].items(), key=lambda kv: -kv[1])[:25]:
        print(f'   {n:4d} {st:10s} {d}')
print()
for g in sorted(al):
    print('## allowances needed', g, dict(al[g].most_common()))
