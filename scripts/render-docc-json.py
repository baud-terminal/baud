#!/usr/bin/env python3
"""Flatten an Apple DocC documentation JSON page into the text note AK cites.

Usage: scripts/render-docc-json.py PAGE.json > OUT.txt

Note AK cites Apple DocC JSON sources (apple-nswindow-*, apple-hig-*) by their
quoted text, and gives "rendered N" -- line N of this script's output -- as a
convenience. The JSON as fetched is what is committed and hashed.
"""
import json, sys
def inl(items, refs):
    out = []
    for it in items or []:
        t = it.get('type')
        if t == 'text': out.append(it['text'])
        elif t == 'codeVoice': out.append('`' + it['code'] + '`')
        elif t in ('emphasis', 'strong', 'newTerm'): out.append(inl(it.get('inlineContent'), refs))
        elif t == 'reference':
            r = refs.get(it.get('identifier'), {})
            out.append(r.get('title') or it.get('identifier', '').split('/')[-1])
        elif t == 'link': out.append(it.get('title', ''))
        elif t == 'image': out.append('[image]')
        else: out.append(inl(it.get('inlineContent'), refs))
    return ''.join(out)
def blocks(bs, refs, depth=0):
    for b in bs or []:
        t = b.get('type')
        if t == 'paragraph': print('  ' * depth + inl(b.get('inlineContent'), refs))
        elif t == 'heading': print('\n' + '#' * b.get('level', 2) + ' ' + b.get('text', ''))
        elif t in ('unorderedList', 'orderedList'):
            for item in b.get('items', []):
                print('  ' * depth + '- ', end=''); blocks(item.get('content'), refs, depth + 1)
        elif t == 'aside': print('  ' * depth + '[' + b.get('name', b.get('style', 'aside')) + ']'); blocks(b.get('content'), refs, depth + 1)
        elif t == 'codeListing': print('    ' + '\n    '.join(b.get('code', [])))
        elif t == 'table':
            for row in b.get('rows', []):
                print(' | '.join(' '.join(inl(c2.get('inlineContent'), refs) for c2 in cell) for cell in row))
        elif t == 'row':
            for col in b.get('columns', []): blocks(col.get('content'), refs, depth)
        elif t == 'tabNavigator':
            for tab in b.get('tabs', []): print('[tab ' + tab.get('title', '') + ']'); blocks(tab.get('content'), refs, depth)
        elif 'content' in b: blocks(b['content'], refs, depth)
        else: print('  ' * depth + '<' + str(t) + '>')
d = json.load(open(sys.argv[1]))
refs = d.get('references', {})
md = d.get('metadata', {})
print('TITLE:', md.get('title'), '| platforms:', [(p.get('name'), p.get('introducedAt')) for p in md.get('platforms', [])])
print('ABSTRACT:', inl(d.get('abstract'), refs))
for sec in d.get('primaryContentSections', []):
    if sec.get('kind') == 'declarations':
        for dec in sec.get('declarations', []): print('DECL:', ''.join(tok.get('text', '') for tok in dec.get('tokens', [])))
    elif sec.get('kind') == 'content': blocks(sec.get('content'), refs)
for s in d.get('sections', []):
    if 'content' in s: blocks(s['content'], refs)
