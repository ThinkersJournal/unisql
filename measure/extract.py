"""Extract SQL string literals from Rust sources and .sql files at a pinned git SHA.
Reads blobs via `git show <sha>:<path>` (never a working tree). Output: JSON lines."""
import json, re, subprocess, sys

KW = re.compile(r'^\s*(?:--[^\n]*\n\s*)*(select|insert|update|delete|with|create|alter|drop|begin|commit|rollback|truncate|copy|show|explain|savepoint|release|lock|do|call|grant|revoke|comment|vacuum|analyze|values|replace|pragma|listen|notify|prepare|execute)\b', re.I)
SET_STMT = re.compile(r'^\s*set\s+(?:local\s+|session\s+)?[\w.]+\s*(=|to)\b', re.I)

ESC = {'n': '\n', 't': '\t', '"': '"', '\\': '\\', "'": "'", 'r': '\r', '0': '\0'}
RAW = re.compile(r'b?r(#*)"')
CHR = re.compile(r"'(?:\\.|[^'\\])'")


def git(repo, *a):
    return subprocess.run(['git', '-C', repo, *a], capture_output=True, check=True).stdout


def rust_literals(src):
    i, n, out = 0, len(src), []
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i)
            i = n if j < 0 else j
            continue
        if src.startswith('/*', i):
            j = src.find('*/', i)
            i = n if j < 0 else j + 2
            continue
        m = RAW.match(src, i) if c in 'br' else None
        if m and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_')):
            h = m.group(1)
            end = src.find('"' + h, m.end())
            if end < 0:
                break
            out.append((src.count('\n', 0, i) + 1, src[m.end():end]))
            i = end + 1 + len(h)
            continue
        if c == '"':
            j, buf = i + 1, []
            while j < n and src[j] != '"':
                if src[j] == '\\':
                    nx = src[j + 1:j + 2]
                    if nx == '\n':
                        j += 2
                        while j < n and src[j] in ' \t\r\n':
                            j += 1
                        continue
                    buf.append(ESC.get(nx, nx))
                    j += 2
                    continue
                buf.append(src[j])
                j += 1
            out.append((src.count('\n', 0, i) + 1, ''.join(buf)))
            i = j + 1
            continue
        if c == "'":
            m2 = CHR.match(src, i)
            i = m2.end() if m2 else i + 1
            continue
        i += 1
    return out


def main():
    repo, sha, corpus = sys.argv[1:4]
    prefixes = sys.argv[4:]
    paths = [p for p in git(repo, 'ls-tree', '-r', '--name-only', sha).decode().split('\n') if p]
    files = [p for p in paths if p.endswith('.rs') or p.endswith('.sql')]
    if prefixes:
        files = [p for p in files if any(p.startswith(x) for x in prefixes)]
    for p in files:
        src = git(repo, 'show', f'{sha}:{p}').decode('utf-8', 'replace')
        if p.endswith('.sql'):
            print(json.dumps({'corpus': corpus, 'sha': sha, 'file': p, 'line': 1, 'sql': src, 'kind': 'sqlfile'}))
            continue
        for ln, t in rust_literals(src):
            if (KW.match(t) or SET_STMT.match(t)) and len(t.split()) >= 2:
                print(json.dumps({'corpus': corpus, 'sha': sha, 'file': p, 'line': ln, 'sql': t, 'kind': 'rust'}))


main()
