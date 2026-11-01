"""Each rule family tried on both compressors: what the code says, checked by running it."""
import json, subprocess, sys, urllib.request

BIN = sys.argv[1]


def headroom(text):
    body = json.dumps({'model': 'claude-sonnet-4-5', 'messages': [
        {'role': 'user', 'content': 'run'},
        {'role': 'assistant', 'content': [{'type': 'tool_use', 'id': 't1', 'name': 'Bash', 'input': {}}]},
        {'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': 't1', 'content': text}]},
    ]}).encode()
    req = urllib.request.Request('http://127.0.0.1:8787/v1/compress', body, {'content-type': 'application/json'})
    answer = json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=120))
    out = answer['messages'][2]['content'][0]['content']
    return out if isinstance(out, str) else text


def rust(text):
    return subprocess.run([BIN], input=text.encode(), capture_output=True, check=True).stdout.decode()


words = 'alpha bravo carol delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango'.split()
w = lambda i: words[i % 20] + '_' + words[i // 20 % 20]
cases = {}

# Errors under and over the 8000-char protection, shape-identical.
small = '\n'.join(f'Traceback line {i}: ValueError in {w(i)}' for i in range(120))
cases['small error output (<8000 c)'] = (small, ['ValueError in tango_alpha', f'Traceback line 60:'])
big = '\n'.join(f'ERROR {i} failed to open {w(i)} code {i % 7}' for i in range(600))
cases['600 distinct errors'] = (big, [f'failed to open {w(n)} ' for n in (0, 25, 150, 300, 450, 599)])
# Stack trace buried in a log.
trace = ['INFO boot ' + w(i) for i in range(300)] + ['Traceback (most recent call last):'] + [f'  File "src/app/{w(i)}.py", line {i}, in run' for i in range(40)] + ['KeyError: missing_invoice'] + ['INFO after ' + w(i) for i in range(300)]
cases['stack trace, 40 frames'] = ('\n'.join(trace), ['KeyError: missing_invoice', w(0) + '.py', w(20) + '.py', w(39) + '.py'])
# Warnings.
warn = ['WARNING deprecated call in ' + w(i) for i in range(200)] + ['done ' + w(i) for i in range(200)]
cases['200 distinct warnings'] = ('\n'.join(warn), ['in ' + w(0), 'in ' + w(100), 'in ' + w(199)])
# Diff: every change counts.
diff = []
for f in range(30):
    diff += [f'diff --git a/{w(f)}.py b/{w(f)}.py', f'--- a/{w(f)}.py', f'+++ b/{w(f)}.py', '@@ -1,12 +1,12 @@']
    diff += [f' context {w(f + c)}' for c in range(5)] + [f'-old_{w(f)} = {f}', f'+new_{w(f)} = {f + 1}'] + [f' after {w(f + c)}' for c in range(5)]
cases['git diff, 30 files'] = ('\n'.join(diff), ['-old_' + w(0), '+new_' + w(15), '+new_' + w(29), 'a/' + w(25) + '.py'])
# Grep with a shared file, the 5-per-file and 30-total caps.
hits = '\n'.join(f'src/{w(i // 10)}.py:{i}: value = compute_{w(i)}()' for i in range(300))
cases['grep, 300 hits, 30 files'] = (hits, ['compute_' + w(n) for n in (0, 7, 155, 299)])
# Prose.
prose = '\n'.join(f'Section {i}. The {w(i)} component stores its state in the {w(i + 3)} table and retries {i % 5} times before it gives up.' for i in range(200))
cases['documentation, 200 paragraphs'] = (prose, [w(n) + ' component' for n in (0, 100, 199)])
# Tabular.
csv = 'id,user,plan\n' + '\n'.join(f'{i},{w(i)},{"pro" if i % 9 else "free"}' for i in range(500))
cases['CSV, 500 rows'] = (csv, [f'250,{w(250)}', f'499,{w(499)}'])
# YAML.
yaml = '\n'.join(f'{w(i)}:\n  host: h{i}.local\n  port: {8000 + i}' for i in range(250))
cases['YAML, 250 blocks'] = (yaml, [f'h{n}.local' for n in (0, 120, 249)])
# Minified.
blob = 'x=' + ''.join(f'{w(i)}={i};' for i in range(1500)) + 'END_MARKER'
cases['minified line'] = (blob, [w(700) + '=700', 'END_MARKER'])
# JSON with an anomaly, a big array of objects.
rows = [{'id': i, 'name': w(i), 'latency': 10 + i % 5, 'status': 'ok'} for i in range(300)]
rows[150].update(latency=9000)
rows[200].update(status='error', detail='disk full')
cases['JSON with an anomaly and an error'] = (json.dumps(rows), ['9000', 'disk full', w(77)])
# Same line repeated.
cases['same line x500'] = ('\n'.join(['still waiting for the server'] * 500), ['still waiting'])
# Progress bar.
cases['progress 0 to 100'] = ('\n'.join(f'Downloading {i}%' for i in range(0, 1001)), ['Downloading 0%', 'Downloading 1000%'])

for name, (text, facts) in cases.items():
    row = [f'{name:34} {len(text) // 4:6}t']
    for fn in (headroom, rust):
        out = fn(text)
        row.append(f'{len(out) // 4:6}t {sum(f in out for f in facts)}/{len(facts)}')
    print(' | '.join(row))
print('\nSample of what Headroom returned for the diff and the documentation:')
print(headroom(cases['documentation, 200 paragraphs'][0])[:300])
print('---')
print(headroom(cases['git diff, 30 files'][0])[:300])
print('---')
print(headroom(cases['JSON with an anomaly and an error'][0])[:300])
