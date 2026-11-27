/* First-call cost in a fresh process, one variant per run, so no variant
   warms another: what each derivation/validation step costs cold. */
#include <CoreFoundation/CoreFoundation.h>
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <xlocale.h>
static double now_us(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW) / 1e3; }
int main(int argc, char **argv) {
    const char *v = argc > 1 ? argv[1] : "";
    char out[128] = "";
    double t0 = now_us();
    if (!strcmp(v, "cflocale")) {
        CFLocaleRef l = CFLocaleCopyCurrent();
        CFStringGetCString(CFLocaleGetIdentifier(l), out, sizeof out, kCFStringEncodingUTF8);
        CFRelease(l);
    } else if (!strcmp(v, "cfprefs")) {
        CFPropertyListRef p = CFPreferencesCopyAppValue(CFSTR("AppleLocale"), kCFPreferencesAnyApplication);
        if (p && CFGetTypeID(p) == CFStringGetTypeID()) CFStringGetCString(p, out, sizeof out, kCFStringEncodingUTF8);
        if (p) CFRelease(p);
    } else if (!strcmp(v, "all") || !strcmp(v, "ctype")) {
        locale_t l = newlocale(!strcmp(v, "all") ? LC_ALL_MASK : LC_CTYPE_MASK, argc > 2 ? argv[2] : "en_US.UTF-8", (locale_t)0);
        snprintf(out, sizeof out, "%s", l ? nl_langinfo_l(CODESET, l) : "NULL");
        if (l) freelocale(l);
    } else if (!strcmp(v, "nothing")) {
        snprintf(out, sizeof out, "-");
    }
    double t1 = now_us();
    printf("%-9s %-14s %8.1f us  -> %s\n", v, argc > 2 ? argv[2] : "", t1 - t0, out);
    return 0;
}
