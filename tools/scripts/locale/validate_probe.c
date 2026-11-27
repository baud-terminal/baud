/* The validator baud would run in the parent before the fork:
   newlocale(mask, name, 0) then nl_langinfo_l(CODESET). Thread-safe, touches
   no process-global locale. Prints the header constants a sys.rs declaration
   and its C probe would need. */
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#ifdef __APPLE__
#include <xlocale.h>
#endif
static void try(const char *name) {
    locale_t all = newlocale(LC_ALL_MASK, name, (locale_t)0);
    locale_t ct = newlocale(LC_CTYPE_MASK, name, (locale_t)0);
    const char *cs_all = all ? nl_langinfo_l(CODESET, all) : "-";
    const char *cs_ct = ct ? nl_langinfo_l(CODESET, ct) : "-";
    int ok = all && strcmp(cs_all, "UTF-8") == 0;
    printf("%-18s LC_ALL_MASK:%-4s CODESET=%-9s | LC_CTYPE_MASK:%-4s CODESET=%-9s => %s\n",
           name[0] ? name : "\"\"", all ? "ok" : "NULL", cs_all, ct ? "ok" : "NULL", cs_ct,
           ok ? "ACCEPT" : "refuse");
    if (all) freelocale(all);
    if (ct) freelocale(ct);
}
int main(void) {
    printf("LC_CTYPE_MASK=%d LC_ALL_MASK=%d CODESET=%d sizeof(locale_t)=%zu sizeof(nl_item)=%zu\n",
           (int)LC_CTYPE_MASK, (int)LC_ALL_MASK, (int)CODESET, sizeof(locale_t), sizeof(nl_item));
    const char *names[] = {"en_US.UTF-8", "he_IL.UTF-8", "C.UTF-8", "en_GB.UTF-8",
                           "en_US", "C", "POSIX", "UTF-8", "bogus_XX.UTF-8",
                           "en_IL.UTF-8", "en_US.utf8", "en_US.UTF8", ""};
    for (unsigned i = 0; i < sizeof names / sizeof *names; i++) try(names[i]);
    return 0;
}
