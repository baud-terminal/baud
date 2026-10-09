// titlebar_probe_sdk14.m -- the Objective-C twin of titlebar_probe.swift,
// built with the devenv shell's nix clang against the nix apple-sdk 14.4 so it
// carries the same LC_BUILD_VERSION (sdk 14.4, minos 14.0) as baud's binary.
// NSTitlebarAccessoryViewController.h line 31 makes trailing placement depend
// on the SDK an app links ("For applications linked on macOS 27.0 or higher").
// Build: clang -O2 -fobjc-arc -mmacosx-version-min=14.0 -framework AppKit \
//          titlebar_probe_sdk14.m -o titlebar_probe_sdk14
#import <AppKit/AppKit.h>

int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        NSWindowStyleMask mask = NSWindowStyleMaskTitled | NSWindowStyleMaskFullSizeContentView |
                                 NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable |
                                 NSWindowStyleMaskResizable;
        NSWindow *win = [[NSWindow alloc] initWithContentRect:NSMakeRect(200, 200, 800, 500)
                                                    styleMask:mask
                                                      backing:NSBackingStoreBuffered
                                                        defer:NO];
        win.titlebarAppearsTransparent = YES;
        win.titleVisibility = NSWindowTitleHidden;
        NSLayoutAttribute attrs[2] = {NSLayoutAttributeTrailing, NSLayoutAttributeRight};
        const char *names[2] = {"trailing", "right"};
        for (int i = 0; i < 2; i++) {
            NSTitlebarAccessoryViewController *vc = [[NSTitlebarAccessoryViewController alloc] init];
            NSView *v = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 96, 10)];
            vc.view = v;
            vc.layoutAttribute = attrs[i];
            [win addTitlebarAccessoryViewController:vc];
            [win layoutIfNeeded];
            [win.contentView.superview layoutSubtreeIfNeeded];
            NSRect r = [v convertRect:v.bounds toView:nil];
            printf("%s: accessory view frame in window (%.1f, %.1f, %.1f, %.1f)\n", names[i],
                   r.origin.x, r.origin.y, r.size.width, r.size.height);
            [win removeTitlebarAccessoryViewControllerAtIndex:win.titlebarAccessoryViewControllers.count - 1];
        }
        NSRect clr = win.contentLayoutRect;
        printf("window frame (%.1f, %.1f); contentLayoutRect (%.1f, %.1f, %.1f, %.1f); strip %.1f\n",
               win.frame.size.width, win.frame.size.height, clr.origin.x, clr.origin.y,
               clr.size.width, clr.size.height, win.frame.size.height - NSMaxY(clr));
        NSWindowButton bs[3] = {NSWindowCloseButton, NSWindowMiniaturizeButton, NSWindowZoomButton};
        for (int i = 0; i < 3; i++) {
            NSButton *b = [win standardWindowButton:bs[i]];
            NSRect r = [b convertRect:b.bounds toView:nil];
            printf("%d: (%.1f, %.1f, %.1f, %.1f)\n", (int)bs[i], r.origin.x, r.origin.y, r.size.width, r.size.height);
        }
        printf("macOS %s\n", NSProcessInfo.processInfo.operatingSystemVersionString.UTF8String);
    }
    return 0;
}
