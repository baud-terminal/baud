import sys
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont
f = TTFont(sys.argv[1]); st = f['GPOS'].table.LookupList.Lookup[7].SubTable[0]
print("lookup 7 SinglePos format", st.Format, "ValueFormat", st.ValueFormat, "value", vars(st.Value))
