#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>

typedef struct {
  double x, y, width, height;
  double screen_left, screen_top, screen_right, screen_bottom;
  uint64_t window_id;
  int32_t foreground; // 1 Finder, 2 Fleqi, 0 other
  bool has_window;
  bool mouse_down;
} FleqiFinderFrame;

// WindowServer metadata is fast, does not activate Finder, and contains only
// on-screen windows. No AppleScript round trip is made by the geometry loop.
FleqiFinderFrame fleqi_finder_frame(void) {
  @autoreleasepool {
    FleqiFinderFrame result = {0};
    NSRunningApplication *front = NSWorkspace.sharedWorkspace.frontmostApplication;
    result.foreground = [front.bundleIdentifier isEqualToString:@"com.apple.finder"] ? 1 :
      (front.processIdentifier == NSProcessInfo.processInfo.processIdentifier ? 2 : 0);
    result.mouse_down = CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft);
    NSRunningApplication *finder = [NSRunningApplication runningApplicationsWithBundleIdentifier:@"com.apple.finder"].firstObject;
    if (!finder) return result;
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID));
    for (NSDictionary *window in windows) {
      if ([window[(id)kCGWindowOwnerPID] intValue] != finder.processIdentifier || [window[(id)kCGWindowLayer] intValue] != 0) continue;
      CGRect bounds;
      if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &bounds)) continue;
      if (bounds.size.width < 200 || bounds.size.height < 100) continue;
      result.x = bounds.origin.x; result.y = bounds.origin.y;
      result.width = bounds.size.width; result.height = bounds.size.height;
      result.window_id = [window[(id)kCGWindowNumber] unsignedLongLongValue];
      CGFloat desktopTop = NSMaxY(NSScreen.screens.firstObject.frame);
      NSPoint center = NSMakePoint(CGRectGetMidX(bounds), desktopTop - CGRectGetMidY(bounds));
      for (NSScreen *screen in NSScreen.screens) if (NSPointInRect(center, screen.frame)) {
        NSRect area = screen.visibleFrame;
        result.screen_left = NSMinX(area); result.screen_right = NSMaxX(area);
        result.screen_top = desktopTop - NSMaxY(area); result.screen_bottom = desktopTop - NSMinY(area);
        break;
      }
      result.has_window = true;
      break;
    }
    return result;
  }
}

static char generationKey;
static char presentationTargetKey;
static char materialKey;
static NSView *materialView(NSView *content, NSString *identifier) {
  for (NSView *view in content.subviews) if ([view.identifier isEqualToString:identifier]) return view;
  return nil;
}
static NSUInteger nextGeneration(NSWindow *window) {
  NSUInteger token = [objc_getAssociatedObject(window, &generationKey) unsignedIntegerValue] + 1;
  objc_setAssociatedObject(window, &generationKey, @(token), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  return token;
}

// 0 solid, 1 native vibrancy, 2 Liquid Glass. Respect accessibility even if the
// app's transparency preference is enabled.
int32_t fleqi_material_kind(bool transparency) {
  if (!transparency || NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceTransparency) return 0;
  if (@available(macOS 26.0, *)) return 2;
  return 1;
}

static NSView *makeMaterial(NSRect frame, int32_t kind, CGFloat radius) {
  if (@available(macOS 26.0, *)) {
    if (kind == 2) {
      NSGlassEffectView *glass = [[NSGlassEffectView alloc] initWithFrame:frame];
      glass.style = NSGlassEffectViewStyleRegular;
      glass.cornerRadius = radius;
      return glass;
    }
  }
  NSVisualEffectView *blur = [[NSVisualEffectView alloc] initWithFrame:frame];
  blur.material = NSVisualEffectMaterialSidebar;
  blur.blendingMode = NSVisualEffectBlendingModeBehindWindow;
  blur.state = NSVisualEffectStateActive;
  blur.wantsLayer = YES;
  blur.layer.cornerRadius = radius;
  blur.layer.masksToBounds = YES;
  return blur;
}

// Invoked only on the AppKit main thread. Materials are behind the existing
// WKWebView; the original content view and event responder chain stay intact.
void fleqi_window_material(void *raw, bool composer, bool transparency, int32_t theme) {
  NSWindow *window = (__bridge NSWindow *)raw;
  NSView *content = window.contentView;
  int32_t kind = fleqi_material_kind(transparency);
  window.appearance = theme == 1 ? [NSAppearance appearanceNamed:NSAppearanceNameAqua] :
    theme == 2 ? [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua] : nil;
  window.opaque = NO;
  window.backgroundColor = NSColor.clearColor;
  window.titlebarAppearsTransparent = YES;
  NSNumber *previous = objc_getAssociatedObject(window, &materialKey);
  if (!previous || previous.intValue != kind) {
    for (NSView *view in [content.subviews copy]) if ([view.identifier hasPrefix:@"fleqi.material."]) [view removeFromSuperview];
    if (kind > 0) {
      NSView *material = makeMaterial(content.bounds, kind, composer ? 16 : 12);
      material.identifier = @"fleqi.material.bar";
      material.autoresizingMask = composer ? NSViewWidthSizable : NSViewWidthSizable | NSViewHeightSizable;
      [content addSubview:material positioned:NSWindowBelow relativeTo:nil];
    }
    objc_setAssociatedObject(window, &materialKey, @(kind), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  }
  if (composer) {
    window.hasShadow = YES;
    window.level = NSFloatingWindowLevel;
    window.collectionBehavior = NSWindowCollectionBehaviorTransient | NSWindowCollectionBehaviorMoveToActiveSpace;
    NSView *bar = materialView(content, @"fleqi.material.bar");
    bar.frame = NSMakeRect(0, 0, content.bounds.size.width, MIN(72, content.bounds.size.height));
    // Floating content owns its surface in the WebView. A separate native plate
    // cannot share its clipping or exit transition and used to leave a 460px ghost.
    [materialView(content, @"fleqi.material.panel") removeFromSuperview];
  }
}

// Position and size must change in a single AppKit transaction. Separate Tauri
// set_size/set_position calls briefly moved the anchored bar by the panel height.
void fleqi_window_frame(void *raw, double x, double y, double width, double height) {
  NSWindow *window = (__bridge NSWindow *)raw;
  CGFloat desktopTop = NSMaxY(NSScreen.screens.firstObject.frame);
  NSRect frame = NSMakeRect(x, desktopTop - y - height, width, height);
  if (NSEqualRects(window.frame, frame)) return;
  [CATransaction begin];
  [CATransaction setDisableActions:YES];
  [window setFrame:frame display:YES animate:NO];
  NSView *bar = materialView(window.contentView, @"fleqi.material.bar");
  bar.frame = NSMakeRect(0, 0, width, MIN(72, height));
  [window invalidateShadow];
  [CATransaction commit];
}

void fleqi_composer_layout(void *raw, double extra) {
  NSWindow *window = (__bridge NSWindow *)raw;
  NSRect frame = window.frame;
  NSRect work = (window.screen ?: NSScreen.mainScreen).visibleFrame;
  CGFloat height = MIN(72 + extra, MAX(72, work.size.height - 16));
  CGFloat bottom = MIN(MAX(frame.origin.y, NSMinY(work) + 8), MAX(NSMinY(work) + 8, NSMaxY(work) - height - 8));
  CGFloat desktopTop = NSMaxY(NSScreen.screens.firstObject.frame);
  fleqi_window_frame(raw, frame.origin.x, desktopTop - bottom - height, frame.size.width, height);
}

void fleqi_window_present(void *raw, bool visible, bool focus, bool return_to_finder, bool reduce_motion) {
  NSWindow *window = (__bridge NSWindow *)raw;
  bool reduce = reduce_motion || NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
  NSNumber *target = objc_getAssociatedObject(window, &presentationTargetKey);
  bool repeated = target && target.boolValue == visible;
  if (!visible && return_to_finder && window.isKeyWindow) {
    NSRunningApplication *finder = [NSRunningApplication runningApplicationsWithBundleIdentifier:@"com.apple.finder"].firstObject;
    [finder activateWithOptions:0];
  }
  if (visible) {
    if (!window.visible) window.alphaValue = reduce ? 1 : 0;
    if (!window.visible) [window orderFront:nil];
    if (focus) {
      [NSRunningApplication.currentApplication activateWithOptions:0];
      [window makeKeyAndOrderFront:nil];
    }
  }
  // Repeated surface events must not restart an in-flight fade. A temporary
  // hide may still supersede a user fade when Finder focus has already changed.
  if (repeated && (visible || !window.visible || !reduce)) return;
  objc_setAssociatedObject(window, &presentationTargetKey, @(visible), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  NSUInteger generation = nextGeneration(window);
  if (reduce) {
    if (!visible) [window orderOut:nil];
    window.alphaValue = 1;
    return;
  }
  [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
    context.duration = visible ? 0.18 : 0.10;
    context.timingFunction = [CAMediaTimingFunction functionWithControlPoints:0.2 :0.8 :0.2 :1];
    [[window animator] setAlphaValue:visible ? 1 : 0];
  } completionHandler:^{
    if ([objc_getAssociatedObject(window, &generationKey) unsignedIntegerValue] != generation) return;
    if (!visible) [window orderOut:nil];
    window.alphaValue = 1;
  }];
}
