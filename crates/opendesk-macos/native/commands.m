#import "native.h"
NSPanel *ODBar;
NSColor *ODColor;
static int repeatKey = -1;
static NSTimeInterval repeatAt;
static CGPoint clickPoint;
static NSTimeInterval clickAt;
static int clickButton = -1, clickCount;
void ODRepeatCancel(void) { repeatKey = -1; }
void ODRepeatTick(void) {
    if (repeatKey < 0 || od_health() != 0)
        return;
    NSTimeInterval now = NSDate.timeIntervalSinceReferenceDate;
    if (now < repeatAt)
        return;
    CGEventRef e = CGEventCreateKeyboardEvent(NULL, repeatKey, true);
    CGEventSetFlags(e, ODFlags);
    CGEventSetIntegerValueField(e, kCGKeyboardEventAutorepeat, 1);
    ODPost(e);
    repeatAt = now + fmax(0.01, NSEvent.keyRepeatInterval);
}
static uint64_t modifier(int key) {
    switch (key) {
    case 54:
    case 55:
        return kCGEventFlagMaskCommand;
    case 56:
    case 60:
        return kCGEventFlagMaskShift;
    case 58:
    case 61:
        return kCGEventFlagMaskAlternate;
    case 59:
    case 62:
        return kCGEventFlagMaskControl;
    default:
        return 0;
    }
}
static CGPoint location(void) {
    CGEventRef e = CGEventCreate(NULL);
    CGPoint p = e ? CGEventGetLocation(e) : CGPointZero;
    if (e)
        CFRelease(e);
    return p;
}
static void bar(NSDictionary *args, BOOL arrival) {
    if (!ODBar) {
        ODBar = [[NSPanel alloc] initWithContentRect:NSZeroRect
                                           styleMask:NSWindowStyleMaskBorderless
                                             backing:NSBackingStoreBuffered
                                               defer:NO];
        ODBar.opaque = NO;
        ODBar.hasShadow = NO;
        ODBar.ignoresMouseEvents = YES;
        ODBar.level = NSStatusWindowLevel;
        ODBar.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                   NSWindowCollectionBehaviorFullScreenAuxiliary;
    }
    NSString *side = args[@"side"];
    double along = [args[@"position"] doubleValue],
           length = 120 * (arrival ? 1 : [args[@"progress"] doubleValue]);
    BOOL vertical = [side isEqual:@"Left"] || [side isEqual:@"Right"];
    NSRect screen = NSScreen.mainScreen.frame;
    double x = vertical ? ([side isEqual:@"Left"] ? 0 : screen.size.width - 4) : along - length / 2;
    double y = vertical ? screen.size.height - along - length / 2
                        : ([side isEqual:@"Top"] ? screen.size.height - 4 : 0);
    [ODBar setFrame:NSMakeRect(screen.origin.x + x, screen.origin.y + y, vertical ? 4 : length,
                               vertical ? length : 4)
            display:YES];
    ODBar.backgroundColor = ODColor ?: NSColor.systemTealColor;
    [ODBar orderFrontRegardless];
    if (arrival)
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 350 * NSEC_PER_MSEC),
                       dispatch_get_main_queue(), ^{
                         [ODBar orderOut:nil];
                       });
}
void ODHandle(id command) {
    NSString *name =
        [command isKindOfClass:NSString.class] ? command : [[command allKeys] firstObject];
    NSDictionary *a = [command isKindOfClass:NSDictionary.class] ? command[name] : @{};
    if ([name isEqual:@"BackendStarted"]) {
        ODBackendStarted();
        return;
    }
    if ([name isEqual:@"ConfigureStrips"]) {
        NSMutableArray *s = [NSMutableArray array];
        for (NSDictionary *strip in a[@"strips"])
            [s addObject:strip[@"side"]];
        ODStrips = s;
        return;
    }
    if ([name isEqual:@"SetReleaseHotkey"]) {
        ODHotkey = a[@"hotkey"];
        return;
    }
    if ([name isEqual:@"StartGrab"]) {
        if (od_health() == 0 && ODCapturePointer()) {
            ODGrab = YES;
        } else
            ODEmit(@"HotkeyPressed");
        return;
    }
    if ([name isEqual:@"LockPointer"]) {
        if (od_health() == 0)
            ODCapturePointer();
        return;
    }
    if ([name isEqual:@"UnlockPointer"] || [name isEqual:@"StopGrab"]) {
        CGPoint p = location();
        NSString *side = ODEdge;
        NSNumber *hint = a[@"hint"];
        if (side) {
            if ([side isEqual:@"Left"])
                p.x = ODBounds.origin.x + 3;
            if ([side isEqual:@"Right"])
                p.x = CGRectGetMaxX(ODBounds) - 4;
            if ([side isEqual:@"Top"])
                p.y = ODBounds.origin.y + 3;
            if ([side isEqual:@"Bottom"])
                p.y = CGRectGetMaxY(ODBounds) - 4;
            if ([hint isKindOfClass:NSNumber.class]) {
                if ([side isEqual:@"Left"] || [side isEqual:@"Right"])
                    p.y = hint.doubleValue;
                else
                    p.x = hint.doubleValue;
            }
        }
        ODRelease();
        CGWarpMouseCursorPosition(p);
        return;
    }
    if ([name isEqual:@"Shutdown"]) {
        ODRelease();
        return;
    }
    if ([name isEqual:@"SetBarStyle"]) {
        NSDictionary *s = a[@"style"];
        ODColor = [NSColor colorWithRed:[s[@"red"] doubleValue] / 255
                                  green:[s[@"green"] doubleValue] / 255
                                   blue:[s[@"blue"] doubleValue] / 255
                                  alpha:[s[@"alpha"] doubleValue] / 255];
        return;
    }
    if ([name isEqual:@"HideProgressBar"]) {
        [ODBar orderOut:nil];
        return;
    }
    if ([name isEqual:@"ShowProgressBar"] || [name isEqual:@"ShowArrivalBar"]) {
        bar(a, [name isEqual:@"ShowArrivalBar"]);
        return;
    }
    BOOL down = [a[@"pressed"] boolValue];
    // Releases still run when permissions/session change; new input is blocked.
    if (od_health() != 0 && down)
        return;
    if ([name isEqual:@"InjectKey"] || [name isEqual:@"InjectPhysicalKey"]) {
        int key = ODVirtualKey([a[@"code"] intValue]);
        if (key < 0)
            return;
        if (down && !modifier(key) && key != 57) {
            repeatKey = key;
            repeatAt = NSDate.timeIntervalSinceReferenceDate + NSEvent.keyRepeatDelay;
        }
        if (!down && repeatKey == key)
            repeatKey = -1;
        if (down)
            [ODKeys addObject:@(key)];
        else
            [ODKeys removeObject:@(key)];
        if (key == 57 && down)
            ODFlags ^= kCGEventFlagMaskAlphaShift;
        ODFlags &= kCGEventFlagMaskAlphaShift;
        for (NSNumber *k in ODKeys)
            ODFlags |= modifier(k.intValue);
        CGEventRef e = CGEventCreateKeyboardEvent(NULL, key, down);
        CGEventSetFlags(e, ODFlags);
        ODPost(e);
        return;
    }
    if ([name isEqual:@"InjectModifiers"]) {
        return;
    }
    if ([name isEqual:@"InjectButton"]) {
        int b = [a[@"code"] intValue] - 272;
        if (b < 0 || b > 31)
            return;
        if (down)
            [ODButtons addObject:@(b)];
        else
            [ODButtons removeObject:@(b)];
        CGEventType type = b == 0   ? (down ? kCGEventLeftMouseDown : kCGEventLeftMouseUp)
                           : b == 1 ? (down ? kCGEventRightMouseDown : kCGEventRightMouseUp)
                                    : (down ? kCGEventOtherMouseDown : kCGEventOtherMouseUp);
        CGPoint p = location();
        NSTimeInterval now = NSDate.timeIntervalSinceReferenceDate;
        if (down) {
            clickCount = clickButton == b && now - clickAt <= NSEvent.doubleClickInterval &&
                                 hypot(p.x - clickPoint.x, p.y - clickPoint.y) < 4
                             ? clickCount + 1
                             : 1;
            clickButton = b;
            clickAt = now;
            clickPoint = p;
        }
        CGEventRef e = CGEventCreateMouseEvent(NULL, type, p, b);
        CGEventSetFlags(e, ODFlags);
        CGEventSetIntegerValueField(e, kCGMouseEventClickState, clickCount);
        ODPost(e);
        return;
    }
    if ([name isEqual:@"InjectAbsoluteMotion"] || [name isEqual:@"InjectMotion"]) {
        if (od_health() != 0)
            return;
        CGPoint p = location();
        double dx = [a[@"dx"] doubleValue], dy = [a[@"dy"] doubleValue];
        BOOL absolute = [name isEqual:@"InjectAbsoluteMotion"];
        if (absolute) {
            p = CGPointMake([a[@"x"] doubleValue], [a[@"y"] doubleValue]);
            ODEdge = nil;
        } else {
            p.x += dx;
            p.y += dy;
        }
        p = ODClamp(p, ODBounds);
        CGEventType type = [ODButtons containsObject:@0]   ? kCGEventLeftMouseDragged
                           : [ODButtons containsObject:@1] ? kCGEventRightMouseDragged
                                                           : kCGEventMouseMoved;
        CGEventRef e =
            CGEventCreateMouseEvent(NULL, type, p, type == kCGEventRightMouseDragged ? 1 : 0);
        CGEventSetFlags(e, ODFlags);
        ODPost(e);
        if (!absolute)
            ODEdges(p, dx, dy);
        return;
    }
    if ([name isEqual:@"InjectAxis"]) {
        if (od_health() != 0)
            return;
        int value = (int)-[a[@"value"] doubleValue];
        BOOL horizontal = [a[@"axis"] isEqual:@"Horizontal"];
        CGEventRef e = CGEventCreateScrollWheelEvent(
            NULL, kCGScrollEventUnitPixel, 2, horizontal ? 0 : value, horizontal ? value : 0);
        CGEventSetFlags(e, ODFlags);
        ODPost(e);
        return;
    }
}
void od_command(const char *json) {
    NSData *data = [[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding];
    id value = [NSJSONSerialization JSONObjectWithData:data
                                               options:NSJSONReadingFragmentsAllowed
                                                 error:nil];
    if (value)
        dispatch_async(dispatch_get_main_queue(), ^{
          ODHandle(value);
        });
}

void od_set_clipboard(const char *mime, const uint8_t *bytes, size_t length) {
    if (length > 10 * 1024 * 1024)
        return;
    NSString *format = [NSString stringWithUTF8String:mime];
    NSData *data = [NSData dataWithBytes:bytes length:length];
    dispatch_async(dispatch_get_main_queue(), ^{
      if (od_health() != 0)
          return;
      NSString *type = [format hasPrefix:@"text/plain"] ? NSPasteboardTypeString
                       : [format isEqual:@"image/png"]  ? NSPasteboardTypePNG
                                                        : nil;
      if (type) {
          [NSPasteboard.generalPasteboard clearContents];
          [NSPasteboard.generalPasteboard setData:data forType:type];
      }
    });
}
