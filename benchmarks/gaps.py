"""Why each thing graphify has is missing from dam-graph. Reference edges to external modules (no source file) are left out."""
import collections, json, os, subprocess, sys

BIN, W = sys.argv[1], sys.argv[2]
LANG = ('.py', '.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.rs', '.java', '.cs', '.go', '.c', '.h', '.cc', '.cpp', '.hpp', '.cxx', '.kt', '.kts')
norm = lambda p: '' if '.min.' in (p or '') else (p or '').replace('\\', '/')


def bare(label):
    return label.strip().lstrip('.').split('(')[0].rsplit('.', 1)[-1].strip()


for repo in sys.argv[3:]:
    cwd = f'{W}/{repo}'
    subprocess.run([BIN, 'update'], cwd=cwd, capture_output=True)
    g = json.load(open(cwd + '/graphify-out/graph.json', encoding='utf-8'))
    nodes = {n['id']: n for n in g['nodes']}
    rd, rc, ri, kinds = set(), set(), set(), {}
    for n in g['nodes']:
        f = norm(n.get('source_file'))
        if n.get('file_type') == 'code' and f.endswith(LANG) and n.get('_origin') == 'ast' and n.get('source_location') != 'L1':
            name = bare(n.get('label', ''))
            if name and not name.endswith(LANG):
                rd.add((f, name))
                kinds[(f, name)] = n.get('source_location')
    for e in g['links']:
        a, b = nodes.get(e['source']), nodes.get(e['target'])
        if not a or not b:
            continue
        fa, fb = norm(a.get('source_file')), norm(b.get('source_file'))
        if e['relation'] == 'calls' and fa.endswith(LANG) and fb.endswith(LANG) and not b.get('label', '').endswith(LANG):
            rc.add((fa, bare(a.get('label', '')), fb, bare(b.get('label', ''))))
        if e['relation'] in ('imports', 'imports_from') and fa.endswith(LANG) and fb.endswith(LANG):
            ri.add((fa, fb))
    files, ns, od, oc, oi = [], [], set(), set(), set()
    for line in open(cwd + '/.dam-graph/graph.tsv', encoding='utf-8'):
        c = line.rstrip('\n').split('\t')
        if c[0] == 'P':
            files.append(c[1])
        elif c[0] == 'N':
            ns.append((c[1], c[2], files[int(c[3])]))
            if c[1] != 'F':
                od.add((files[int(c[3])], c[2]))
        elif c[0] == 'E':
            a, b = ns[int(c[1])], ns[int(c[2])]
            if c[3] == 'c' and a[0] != 'F':
                oc.add((a[2], a[1], b[2], b[1]))
            if c[3] == 'i':
                oi.add((a[2], b[2]))
    print(f'\n=== {repo}: defs {len(rd & od)}/{len(rd)}, calls {len(rc & oc)}/{len(rc)} (+{len(oc - rc)} extra), imports {len(ri & oi)}/{len(ri)} (+{len(oi - ri)} extra)')
    miss = rd - od
    print('missed defs by ext', collections.Counter(os.path.splitext(f)[1] for f, _ in miss).most_common(6))
    for ext in ('.rs', '.ts', '.tsx', '.js', '.java', '.py', '.mjs'):
        s = sorted(x for x in miss if x[0].endswith(ext))[:14]
        if s:
            print(' ', ext, [f'{os.path.basename(f)}:{n}@{kinds[(f, n)]}' for f, n in s])
    why = collections.Counter()
    names = collections.Counter(n for _, n in od)
    samples = collections.defaultdict(list)
    for fa, ca, fb, cb in rc - oc:
        if (fa, ca) not in od:
            k = 'caller not a def here'
        elif (fb, cb) not in od:
            k = 'callee not a def here'
        elif fa == fb:
            k = 'same file'
        elif names[cb] == 1:
            k = 'unique name'
        else:
            k = 'ambiguous name'
        ext = os.path.splitext(fa)[1]
        why[(ext, k)] += 1
        if len(samples[(ext, k)]) < 5:
            samples[(ext, k)].append(f'{os.path.basename(fa)}:{ca}->{os.path.basename(fb)}:{cb}')
    for key, n in why.most_common(12):
        print(' calls', key, n, samples[key])
    extra = sorted(oc - rc)
    print(' extra sample', [f'{os.path.basename(a)}:{b}->{os.path.basename(c)}:{d}' for a, b, c, d in extra[:: max(1, len(extra) // 10)]][:10])
    print(' missed imports', [f'{os.path.basename(a)}->{b}' for a, b in sorted(ri - oi)[:8]])
    print(' extra imports', [f'{os.path.basename(a)}->{b}' for a, b in sorted(oi - ri)[:8]])
