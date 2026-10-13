import sys, html, re
t = open(sys.argv[1], encoding='utf-8', errors='replace').read()
t = re.sub(r'(?is)<(script|style).*?</\1>', ' ', t)
t = re.sub(r'(?i)<br\s*/?>|</p>|</li>|</h\d>|</tr>|</pre>', '\n', t)
t = re.sub(r'<[^>]+>', '', t)
t = html.unescape(t)
t = re.sub(r'[ \t]+', ' ', t)
print(re.sub(r'\n\s*\n+', '\n', t))
