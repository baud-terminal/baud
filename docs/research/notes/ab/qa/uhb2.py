import sys
sys.path.insert(0, sys.argv[3]); import uhb_lib as U
a = [l.rstrip('\n').split('\t') for l in open(sys.argv[1])]; b = [l.rstrip('\n').split('\t') for l in open(sys.argv[2])]
d = sum(1 for x, y in zip(a, b) if U.shape(x[0], int(x[1]), [int(h, 16) for h in x[2].split()], 'Latn') != U.shape(y[0], int(y[1]), [int(h, 16) for h in y[2].split()]))
print("lines", len(a), "hb(original, forced Latn) != hb(no-dflt copy, guessed):", d)
