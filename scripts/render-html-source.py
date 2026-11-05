#!/usr/bin/env python3
"""Render a fetched HTML source page to line-numbered text.

Usage: scripts/render-html-source.py MODE IN.html > OUT.txt

A citation of an HTML source as "rendered N" means line N of this script's output for the page as committed (or, for an external
page, as fetched from its pinned url). The output is reproducible byte for byte;
the page itself is what is committed and hashed.

The note cites HTML sources by line of these renderings, so the mode used for
each file is part of the citation (see manifest-proposed.toml):
  guide  Apple Terminal User Guide pages
  posix  The Open Group's tcgetpgrp page
  faq    the xterm FAQ and the iTerm2 documentation pages
  uax9   UAX #9 revision 52
"""
import html, re, sys

BREAKS = {
    'guide': r'<(br|/p|/li|/tr|/h[1-6]|/td|/th|/div)[^>]*>',
    'posix': r'<(br|/p|/li|/h[1-6]|/dt|/dd|/blockquote|/pre)[^>]*>',
    'faq':   r'<(br|/p|/li|/tr|/h[1-6]|/dt|/dd|/blockquote|/pre|/table|/div)[^>]*>',
    'uax9':  r'<(br|/p|/li|/tr|/h[1-6]|/dt|/dd|/blockquote|/pre|/table)[^>]*>',
}

def render(mode, raw):
    s = raw.decode('utf-8', 'strict' if mode == 'guide' else 'replace')
    s = re.sub(r'<(script|style)[^>]*>.*?</\1>', '', s, flags=re.S)
    t = re.sub(BREAKS[mode], '\n', s)
    if mode == 'uax9':
        t = re.sub(r'</t[dh]>', ' | ', t)
    t = html.unescape(re.sub(r'<[^>]+>', '', t))
    lines = t.split('\n')
    if mode == 'uax9':
        lines = [re.sub(r'\s+', ' ', l).strip() for l in lines]
    elif mode == 'faq':
        lines = [re.sub(r'[ \t]+', ' ', l).strip() for l in lines]
    else:
        lines = [l.strip() for l in lines]
    return '\n'.join(l for l in lines if l)

if __name__ == '__main__':
    out = render(sys.argv[1], open(sys.argv[2], 'rb').read())
    trailing = '' if sys.argv[1] == 'guide' else '\n'
    sys.stdout.write(out + trailing)
