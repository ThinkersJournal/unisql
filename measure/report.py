"""Write untranslatable-by-tier.txt: every non-exact statement, grouped by tier then reason,
with file:line at the pinned SHA. Input: *.out.jsonl produced by classify."""
import collections
import json
import re
import sys

sys.argv = ['x']
exec(open('summarise.py').read().replace('\nmain()\n', '\n'))

rows = [json.loads(l) for c in ('auth', 'lightbulb', 'humboldt') for l in open(c + '.out.jsonl')]
rows = [r for r in rows if not is_prose(r)]
out = []
for tier in ('refused', 'conditional', 'emulated', 'unparsed'):
    out.append(f'=== statements whose WORST tier is {tier} ===')
    per_reason = collections.defaultdict(list)
    n = 0
    for r in rows:
        t = r['tier'] if r['parsed'] else 'unparsed'
        if t != tier:
            continue
        n += 1
        reasons = [x.split(':', 1)[1] for x in r.get('reasons', []) if x.split(':', 1)[0] == tier] or [r.get('error', '')[:80]]
        for why in reasons:
            per_reason[why].append(f"{group(r)} {r['file']}:{r['line']} [{cat(r)}]")
    out.append(f'  statements: {n}')
    for why, locs in sorted(per_reason.items(), key=lambda kv: -len(kv[1])):
        out.append(f'  {len(locs):4d}  {why}')
        for loc in locs[:6]:
            out.append(f'          {loc}')
        if len(locs) > 6:
            out.append(f'          ... +{len(locs) - 6} more')
    out.append('')
open('untranslatable-by-tier.txt', 'w', encoding='utf-8').write('\n'.join(out))
print('\n'.join(out[:60]))
