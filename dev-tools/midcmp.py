"""Real mid-size outputs, the only size the mod compresses, through both. Every output comes from a real
command on a real repository or a real saved tool result cut to size. Nothing leaves this machine."""
import glob, json, os, re, statistics, subprocess, sys, time, urllib.request

BIN, W = sys.argv[1], sys.argv[2]
home = os.path.expanduser('~')
CAP = 28_000
samples = {}


def cut(text):
    text = text[:CAP]
    return text[:text.rfind('\n') + 1] if '\n' in text else text


def add(name, cmd, cwd):
    run = subprocess.run(cmd, cwd=cwd, capture_output=True, shell=isinstance(cmd, str))
    text = cut(run.stdout.decode('utf-8', 'replace') + run.stderr.decode('utf-8', 'replace'))
    if len(text) >= 6000:
        samples[name] = text


for repo in ('TokenEconomizer', 'Optimism', 'headroom'):
    d = f'{W}/{repo}'
    add(f'{repo}: git log', ['git', 'log', '--stat', '-40'], d)
    add(f'{repo}: git log oneline', ['git', 'log', '--format=%h %an %ad %s', '--date=iso', '-300'], d)
    add(f'{repo}: git ls-files', ['git', 'ls-files'], d)
    add(f'{repo}: git diff', ['git', 'diff', 'HEAD~8', 'HEAD'], d)
    add(f'{repo}: ls -l', 'ls -lR --color=never', d)
    add(f'{repo}: grep def/class', ['git', 'grep', '-n', '-E', 'class |function |def '], d)
    add(f'{repo}: grep import', ['git', 'grep', '-n', 'import'], d)
add('TokenEconomizer: unit tests', 'python3 -m pytest -q --co 2>&1 || true', f'{W}/TokenEconomizer')
PY = sys.executable
add('TokenEconomizer: unittest listing', [PY, '-m', 'unittest', 'discover', '-s', 'tests', '-v'], f'{W}/TokenEconomizer')
add('cargo tree', ['cargo', 'tree'], f'{W}/headroom')
add('cargo build log', ['cargo', 'metadata', '--format-version', '1', '--no-deps'], f'{W}/headroom')
for n, path in enumerate(sorted(glob.glob(home + '/.claude/projects/*/*/tool-results/*.txt'))[:25]):
    text = cut(open(path, encoding='utf-8', errors='ignore').read())
    if len(text) >= 6000 and 'Headroom condensed' not in text:
        samples[f'saved result {n}: {text[:24]!r}'] = text


def headroom(text):
    body = json.dumps({'model': 'claude-sonnet-4-5', 'messages': [
        {'role': 'user', 'content': 'run'},
        {'role': 'assistant', 'content': [{'type': 'tool_use', 'id': 't1', 'name': 'Bash', 'input': {}}]},
        {'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': 't1', 'content': text}]},
    ]}).encode()
    req = urllib.request.Request('http://127.0.0.1:8787/v1/compress', body, {'content-type': 'application/json'})
    start = time.perf_counter()
    answer = json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=120))
    out = answer['messages'][2]['content'][0]['content']
    return (out if isinstance(out, str) else text), time.perf_counter() - start


def rust(text):
    start = time.perf_counter()
    out = subprocess.run([BIN], input=text.encode(), capture_output=True, check=True).stdout.decode()
    return out, time.perf_counter() - start


def rust_lines(text, out):
    if out == text:
        return text
    decoded = subprocess.run([BIN, '--decode'], input=out.encode(), capture_output=True, check=True).stdout.decode()
    return decoded


def is_exact(text, out, name):
    if name == 'rust':
        return [l.rstrip() for l in text.splitlines()] == [l.rstrip() for l in rust_lines(text, out).splitlines()]
    return None


totals = {'headroom': [0, 0, 0, 0, []], 'rust': [0, 0, 0, 0, []]}
print(f'{"output":44} {"tokens":>7} {"headroom":>9} {"kept":>5} {"rust":>7} {"kept":>5}')
bad = 0
for name, text in samples.items():
    wanted = {l.strip() for l in text.splitlines() if len(l.strip()) > 3}
    row = [f'{name[:44]:44} {len(text) // 4:7}']
    for label, fn in (('headroom', headroom), ('rust', rust)):
        out, took = fn(text)
        # What a reader can actually get back from this result alone.
        readable = rust_lines(text, out) if label == 'rust' else out
        got = {l.strip() for l in readable.splitlines()}
        kept = len(wanted & got)
        if label == 'rust' and kept != len(wanted):
            bad += 1
        t = totals[label]
        t[0] += len(text); t[1] += min(len(out), len(text)); t[2] += kept; t[3] += len(wanted); t[4].append(took)
        row.append(f'{len(out) // 4:8} {100 * kept / max(1, len(wanted)):4.0f}%')
    print(' '.join(row))
print(f'\n{len(samples)} outputs; rust outputs not recovering every line: {bad}')
for label, t in totals.items():
    print(f'{label}: {t[0] // 4} -> {t[1] // 4} tokens, {100 * (t[0] - t[1]) / t[0]:.1f}% saved; lines recoverable from the result {t[2]}/{t[3]} ({100 * t[2] / t[3]:.1f}%); median {statistics.median(t[4]) * 1000:.0f} ms, max {max(t[4]) * 1000:.0f} ms')
