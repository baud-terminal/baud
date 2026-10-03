/* What CoreFoundation says the user's locale is, with no LANG in the
   environment: the derivation a Dock-launched baud would run. Plain C over
   CoreFoundation, the shape a `#[link(name = "CoreFoundation", kind =
   "framework")]` extern block in baud-app would have. */
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
static void put(const char *label, CFStringRef s) {
    char buf[128] = "(null)";
    if (s && !CFStringGetCString(s, buf, sizeof buf, kCFStringEncodingUTF8)) snprintf(buf, sizeof buf, "(too long)");
    printf("%s=%s ", label, buf);
}
int main(void) {
    const char *lang = getenv("LANG");
    printf("env LANG=%s | ", lang ? lang : "(unset)");
    CFLocaleRef loc = CFLocaleCopyCurrent();
    put("identifier", CFLocaleGetIdentifier(loc));
    put("language", (CFStringRef)CFLocaleGetValue(loc, kCFLocaleLanguageCode));
    put("country", (CFStringRef)CFLocaleGetValue(loc, kCFLocaleCountryCode));
    printf("\n");
    CFRelease(loc);
    printf("kCFStringEncodingUTF8=0x%x sizeof(CFIndex)=%zu\n", (unsigned)kCFStringEncodingUTF8, sizeof(CFIndex));
    return 0;
}
