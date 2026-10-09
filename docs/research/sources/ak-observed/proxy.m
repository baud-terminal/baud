// Does NSWindow give a document (proxy) icon button when the title is hidden?
// An offscreen, fully transparent window; reads the standard button's state.
#import <AppKit/AppKit.h>
static void hierarchy_hidden(NSView *v, int *anyHidden) { for (NSView *p = v; p; p = p.superview) if (p.hidden) *anyHidden = 1; }
int main(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    for (int hidden = 0; hidden < 2; hidden++) for (int transparent = 0; transparent < 2; transparent++) {
      NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(-20000, -20000, 400, 300)
        styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable|NSWindowStyleMaskResizable|NSWindowStyleMaskFullSizeContentView
        backing:NSBackingStoreBuffered defer:NO];
      w.releasedWhenClosed = NO;
      w.alphaValue = 0.0;
      w.title = @"probe-title";
      w.subtitle = @"probe-subtitle";
      w.titleVisibility = hidden ? NSWindowTitleHidden : NSWindowTitleVisible;
      w.titlebarAppearsTransparent = transparent;
      w.representedURL = [NSURL fileURLWithPath:@"/tmp"];
      [w orderBack:nil];
      [w.contentView.superview layoutSubtreeIfNeeded];
      [w displayIfNeeded];
      NSButton *b = [w standardWindowButton:NSWindowDocumentIconButton];
      int anyHidden = 0; if (b) hierarchy_hidden(b, &anyHidden);
      // Any text field in the frame showing the title or subtitle?
      __block int titleShown = 0, subtitleShown = 0;
      __block void (^walk)(NSView *);
      walk = ^(NSView *v) {
        if ([v isKindOfClass:[NSTextField class]]) {
          NSTextField *t = (NSTextField *)v; int h = 0; hierarchy_hidden(t, &h);
          if (!h && t.alphaValue > 0 && [t.stringValue isEqualToString:@"probe-title"]) titleShown = 1;
          if (!h && t.alphaValue > 0 && [t.stringValue isEqualToString:@"probe-subtitle"]) subtitleShown = 1;
        }
        for (NSView *s in v.subviews) walk(s);
      };
      walk(w.contentView.superview);
      printf("titleVisibility=%s transparent=%d: docButton=%s hidden=%d inHiddenAncestor=%d alpha=%.1f frame=(%.0f,%.0f %.0fx%.0f) image=%s | title field shown=%d subtitle field shown=%d\n",
        hidden ? "Hidden " : "Visible", transparent, b ? "yes" : "no", b ? b.hidden : -1, anyHidden, b ? b.alphaValue : -1.0,
        b ? b.frame.origin.x : 0, b ? b.frame.origin.y : 0, b ? b.frame.size.width : 0, b ? b.frame.size.height : 0,
        (b && b.image) ? "yes" : "no", titleShown, subtitleShown);
      [w close];
    }
  }
  return 0;
}
