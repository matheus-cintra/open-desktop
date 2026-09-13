#import "native.h"
#import <IOKit/hidsystem/IOLLEvent.h>
void (*ODEvent)(const char *);
void (*ODClipboard)(const char *, const uint8_t *, size_t);
BOOL ODGrab = NO, ODLocked = NO, ODSuspended = NO;
CFMachPortRef ODTap;
CGRect ODBounds;
NSArray *ODStrips = @[];
NSString *ODEdge;
uint64_t ODFlags;
NSMutableSet *ODKeys, *ODButtons;
NSDictionary *ODHotkey;
const int64_t ODMarker = 0x4f4445534b;
static atomic_int health = 1;
static BOOL hidden, ready, disconnected;
static CGPoint capturePosition;
static BOOL captureDriftReported;
void ODBackendStarted(void) { ready = NO; }
static NSInteger clipboardCount = -1;
static CGRect previousBounds;
static const int keys[][2] = {
    {0, 30},    {1, 31},    {2, 32},   {3, 33},    {4, 35},   {5, 34},    {6, 44},    {7, 45},
    {8, 46},    {9, 47},    {10, 86},  {11, 48},   {12, 16},  {13, 17},   {14, 18},   {15, 19},
    {16, 21},   {17, 20},   {18, 2},   {19, 3},    {20, 4},   {21, 5},    {22, 7},    {23, 6},
    {24, 13},   {25, 10},   {26, 8},   {27, 12},   {28, 9},   {29, 11},   {30, 27},   {31, 24},
    {32, 22},   {33, 26},   {34, 23},  {35, 25},   {36, 28},  {37, 38},   {38, 36},   {39, 40},
    {40, 37},   {41, 39},   {42, 43},  {43, 51},   {44, 53},  {45, 49},   {46, 50},   {47, 52},
    {48, 15},   {49, 57},   {50, 41},  {51, 14},   {53, 1},   {54, 126},  {55, 125},  {56, 42},
    {57, 58},   {58, 56},   {59, 29},  {60, 54},   {61, 100}, {62, 97},   {65, 83},   {67, 55},
    {69, 78},   {71, 69},   {75, 98},  {76, 96},   {78, 74},  {81, 117},  {82, 82},   {83, 79},
    {84, 80},   {85, 81},   {86, 75},  {87, 76},   {88, 77},  {89, 71},   {91, 72},   {92, 73},
    {93, 124},  {94, 89},   {96, 63},  {97, 64},   {98, 65},  {99, 61},   {100, 66},  {101, 67},
    {103, 87},  {105, 99},  {107, 70}, {109, 68},  {111, 88}, {113, 119}, {114, 110}, {115, 102},
    {116, 104}, {117, 111}, {118, 62}, {119, 107}, {120, 60}, {121, 109}, {122, 59},  {123, 105},
    {124, 106}, {125, 108}, {126, 103}};
int ODVirtualKey(int code) {
    for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
        if (keys[i][1] == code)
            return keys[i][0];
    return -1;
}
int ODEvdev(int key) {
    for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
        if (keys[i][0] == key)
            return keys[i][1];
    return -1;
}
BOOL ODModifierPressed(int key, uint64_t flags) {
    uint64_t side, pair, generic;
    switch (key) {
    case 59:
    case 62:
        side = key == 59 ? NX_DEVICELCTLKEYMASK : NX_DEVICERCTLKEYMASK;
        pair = NX_DEVICELCTLKEYMASK | NX_DEVICERCTLKEYMASK;
        generic = kCGEventFlagMaskControl;
        break;
    case 56:
    case 60:
        side = key == 56 ? NX_DEVICELSHIFTKEYMASK : NX_DEVICERSHIFTKEYMASK;
        pair = NX_DEVICELSHIFTKEYMASK | NX_DEVICERSHIFTKEYMASK;
        generic = kCGEventFlagMaskShift;
        break;
    case 55:
    case 54:
        side = key == 55 ? NX_DEVICELCMDKEYMASK : NX_DEVICERCMDKEYMASK;
        pair = NX_DEVICELCMDKEYMASK | NX_DEVICERCMDKEYMASK;
        generic = kCGEventFlagMaskCommand;
        break;
    case 58:
    case 61:
        side = key == 58 ? NX_DEVICELALTKEYMASK : NX_DEVICERALTKEYMASK;
        pair = NX_DEVICELALTKEYMASK | NX_DEVICERALTKEYMASK;
        generic = kCGEventFlagMaskAlternate;
        break;
    case 57:
        return (flags & kCGEventFlagMaskAlphaShift) != 0;
    default:
        return NO;
    }
    // Prefer side-specific hardware flags. Synthetic events may only carry
    // the generic modifier bit. Never query a global state table from the tap.
    return (flags & generic) && ((flags & pair) ? (flags & side) != 0 : YES);
}
void ODEmit(id value) {
    if (!ODEvent)
        return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:value
                                                   options:NSJSONWritingFragmentsAllowed
                                                     error:nil];
    if (data)
        ODEvent([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String);
}
void ODPost(CGEventRef e) {
    if (!e)
        return;
    CGEventSetIntegerValueField(e, kCGEventSourceUserData, ODMarker);
    CGEventPost(kCGSessionEventTap, e);
    CFRelease(e);
}
BOOL ODCapturePointer(void) {
    if (!disconnected) {
        CGEventRef current = CGEventCreate(NULL);
        capturePosition = current ? CGEventGetLocation(current) : CGPointZero;
        if (current)
            CFRelease(current);
        captureDriftReported = NO;
        if (!hidden)
            hidden = CGDisplayHideCursor(CGMainDisplayID()) == kCGErrorSuccess;
        CGError result = CGAssociateMouseAndMouseCursorPosition(false);
        NSLog(@"OD capture: HID tap=%d active=%d hide=%d detach=%d", ODTap != NULL, NSApp.isActive,
              hidden, result);
        if (result != kCGErrorSuccess) {
            ODRelease();
            ODEmit(@{@"Fatal" : @{@"message" : @"macOS refused to detach the local cursor"}});
            return NO;
        }
        disconnected = YES;
    }
    ODLocked = YES;
    return YES;
}
void ODRelease(void) {
    if (disconnected) {
        NSLog(@"OD capture released");
        CGAssociateMouseAndMouseCursorPosition(true);
        disconnected = NO;
    }
    ODRepeatCancel();
    ODGrab = NO;
    ODLocked = NO;
    ODEdge = nil;
    if (hidden) {
        CGDisplayShowCursor(CGMainDisplayID());
        hidden = NO;
    }
    for (NSNumber *key in [ODKeys copy])
        ODPost(CGEventCreateKeyboardEvent(NULL, key.unsignedShortValue, false));
    CGEventRef pos = CGEventCreate(NULL);
    CGPoint p = pos ? CGEventGetLocation(pos) : CGPointZero;
    if (pos)
        CFRelease(pos);
    for (NSNumber *button in [ODButtons copy]) {
        int b = button.intValue;
        ODPost(CGEventCreateMouseEvent(NULL,
                                       b == 0   ? kCGEventLeftMouseUp
                                       : b == 1 ? kCGEventRightMouseUp
                                                : kCGEventOtherMouseUp,
                                       p, b));
    }
    [ODKeys removeAllObjects];
    [ODButtons removeAllObjects];
    ODFlags = 0;
    [ODBar orderOut:nil];
}
CGPoint ODClamp(CGPoint p, CGRect bounds) {
    p.x = fmax(CGRectGetMinX(bounds), fmin(CGRectGetMaxX(bounds) - 1, p.x));
    p.y = fmax(CGRectGetMinY(bounds), fmin(CGRectGetMaxY(bounds) - 1, p.y));
    return p;
}
NSString *ODSideAt(CGPoint p, double dx, double dy, CGRect bounds) {
    NSString *side = nil;
    if (p.x <= CGRectGetMinX(bounds) + 0.5 && dx < 0)
        side = @"Left";
    else if (p.x >= CGRectGetMaxX(bounds) - 1 && dx > 0)
        side = @"Right";
    else if (p.y <= CGRectGetMinY(bounds) + 0.5 && dy < 0)
        side = @"Top";
    else if (p.y >= CGRectGetMaxY(bounds) - 1 && dy > 0)
        side = @"Bottom";
    return side;
}
void ODEdges(CGPoint p, double dx, double dy) {
    if (ODGrab || atomic_load(&health) != 0)
        return;
    NSString *side = ODSideAt(p, dx, dy, ODBounds);
    if (side && ![ODStrips containsObject:side])
        side = nil;
    if (ODEdge && ![ODEdge isEqual:side] && !ODLocked) {
        ODEmit(@{@"EdgeLeft" : @{@"side" : ODEdge}});
        ODEdge = nil;
    }
    if (side && (!ODEdge || !ODLocked)) {
        ODEdge = side;
        ODEmit(@{
            @"EdgeEntered" : @{
                @"side" : side,
                @"output" : @"main",
                @"position" : @(([side isEqual:@"Left"] || [side isEqual:@"Right"]) ? p.y : p.x)
            }
        });
    }
}
static CGEventRef tap(CGEventTapProxy proxy, CGEventType type, CGEventRef e, void *info) {
    (void)proxy;
    (void)info;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        ODRelease();
        ODEmit(@"HotkeyPressed");
        CGEventTapEnable(ODTap, true);
        return e;
    }
    if (CGEventGetIntegerValueField(e, kCGEventSourceUserData) == ODMarker)
        return e;
    // Hardware only; never treat CGEventPost events as a physical takeover.
    if (CGEventGetIntegerValueField(e, kCGEventSourceUnixProcessID) == 0) {
        static double lastMotion, distance;
        static BOOL motionSent;
        BOOL activity = (type == kCGEventKeyDown && !CGEventGetIntegerValueField(e, kCGKeyboardEventAutorepeat)) || type == kCGEventLeftMouseDown || type == kCGEventRightMouseDown || type == kCGEventOtherMouseDown;
        if (type == kCGEventFlagsChanged) {
            activity = ODModifierPressed((int)CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode), CGEventGetFlags(e));
        }
        if (type == kCGEventMouseMoved || type == kCGEventLeftMouseDragged || type == kCGEventRightMouseDragged) {
            double now = NSProcessInfo.processInfo.systemUptime;
            if (now - lastMotion >= 0.250) { distance = 0; motionSent = NO; }
            lastMotion = now;
            distance += fabs(CGEventGetDoubleValueField(e, kCGMouseEventDeltaX)) + fabs(CGEventGetDoubleValueField(e, kCGMouseEventDeltaY));
            if (distance >= 12 && !motionSent) { activity = YES; motionSent = YES; }
        }
        if (activity && !ODSuspended && !ODLocked) ODEmit(@"PhysicalActivity");
    }

    if (atomic_load(&health) != 0)
        return e;
    uint64_t flags = CGEventGetFlags(e);
    if (type == kCGEventKeyDown && CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode) == 53 &&
        (flags & (kCGEventFlagMaskControl | kCGEventFlagMaskAlternate)) ==
            (kCGEventFlagMaskControl | kCGEventFlagMaskAlternate)) {
        if (ODGrab || ODLocked) {
            ODRelease();
            ODEmit(@"HotkeyPressed");
            return NULL;
        }
    }
    if (type == kCGEventMouseMoved || type == kCGEventLeftMouseDragged ||
        type == kCGEventRightMouseDragged || type == kCGEventOtherMouseDragged) {
        double dx = CGEventGetDoubleValueField(e, kCGMouseEventDeltaX),
               dy = CGEventGetDoubleValueField(e, kCGMouseEventDeltaY);
        if (ODGrab || ODLocked) {
            // Background accessory apps cannot rely on cursor disassociation.
            // Warping does not generate input events; retain the physical deltas
            // above for the peer and pin the local cursor before consuming input.
            CGError result = CGWarpMouseCursorPosition(capturePosition);
            if (result != kCGErrorSuccess) {
                NSLog(@"OD capture: cursor pin failed=%d", result);
                ODRelease();
                ODEmit(@"HotkeyPressed");
                return NULL;
            }
            CGEventSetLocation(e, capturePosition);
        }
        ODEdges(CGEventGetLocation(e), dx, dy);
        if (ODGrab || ODLocked || ODEdge)
            ODEmit(@{@"RelativeMotion" : @{@"dx" : @(dx), @"dy" : @(dy)}});
        return (ODGrab || ODLocked) ? NULL : e;
    }
    if (!ODGrab)
        return e;
    if (type == kCGEventKeyDown || type == kCGEventKeyUp || type == kCGEventFlagsChanged) {
        int key = (int)CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode), code = ODEvdev(key);
        BOOL pressed =
            type == kCGEventFlagsChanged ? ODModifierPressed(key, flags) : type == kCGEventKeyDown;
        if (code >= 0 && !CGEventGetIntegerValueField(e, kCGKeyboardEventAutorepeat))
            ODEmit(@{@"Key" : @{@"code" : @(code), @"pressed" : @(pressed)}});
        return NULL;
    }
    if (type == kCGEventScrollWheel) {
        for (int axis = 0; axis < 2; axis++) {
            double v = CGEventGetDoubleValueField(e, axis ? kCGScrollWheelEventPointDeltaAxis2
                                                          : kCGScrollWheelEventPointDeltaAxis1);
            if (v)
                ODEmit(@{
                    @"Axis" : @{
                        @"axis" : axis ? @"Horizontal" : @"Vertical",
                        @"value" : @(-v),
                        @"value120" : @0,
                        @"source" : @"Continuous"
                    }
                });
        }
        return NULL;
    }
    if (type == kCGEventLeftMouseDown || type == kCGEventLeftMouseUp ||
        type == kCGEventRightMouseDown || type == kCGEventRightMouseUp ||
        type == kCGEventOtherMouseDown || type == kCGEventOtherMouseUp) {
        int b = (int)CGEventGetIntegerValueField(e, kCGMouseEventButtonNumber);
        BOOL down = type == kCGEventLeftMouseDown || type == kCGEventRightMouseDown ||
                    type == kCGEventOtherMouseDown;
        ODEmit(@{@"Button" : @{@"code" : @(272 + b), @"pressed" : @(down)}});
        return NULL;
    }
    return e;
}
int od_health(void) { return atomic_load(&health); }
void ODTick(void) {
    @autoreleasepool {
        uint32_t count = 0;
        CGGetActiveDisplayList(0, NULL, &count);
        NSDictionary *session = CFBridgingRelease(CGSessionCopyCurrentDictionary());
        BOOL inactive = !session ||
                        ![session[(__bridge NSString *)kCGSessionOnConsoleKey] boolValue] ||
                        [session[@"CGSSessionScreenIsLocked"] boolValue];
        BOOL allowed = CGPreflightListenEventAccess() && CGPreflightPostEventAccess();
        BOOL blocked = inactive || ODSuspended || IsSecureEventInputEnabled() || count != 1;
        if (allowed && !ODTap) {
            CGEventMask mask =
                CGEventMaskBit(kCGEventMouseMoved) | CGEventMaskBit(kCGEventLeftMouseDragged) |
                CGEventMaskBit(kCGEventRightMouseDragged) |
                CGEventMaskBit(kCGEventOtherMouseDragged) | CGEventMaskBit(kCGEventLeftMouseDown) |
                CGEventMaskBit(kCGEventLeftMouseUp) | CGEventMaskBit(kCGEventRightMouseDown) |
                CGEventMaskBit(kCGEventRightMouseUp) | CGEventMaskBit(kCGEventOtherMouseDown) |
                CGEventMaskBit(kCGEventOtherMouseUp) | CGEventMaskBit(kCGEventScrollWheel) |
                CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventKeyUp) |
                CGEventMaskBit(kCGEventFlagsChanged);
            // Filter physical movement before the window server applies it to the local cursor.
            // A session tap can consume delivery to apps after the cursor has already moved.
            ODTap = CGEventTapCreate(kCGHIDEventTap, kCGHeadInsertEventTap,
                                     kCGEventTapOptionDefault, mask, tap, NULL);
            NSLog(@"OD input: HID event tap creation %@", ODTap ? @"succeeded" : @"failed");
            if (ODTap) {
                CFRunLoopSourceRef source = CFMachPortCreateRunLoopSource(NULL, ODTap, 0);
                CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
                CFRelease(source);
            }
        }
        int value = !allowed || !ODTap ? 1 : blocked ? 2 : 0;
        ODPermissionTick(value);
        if (atomic_exchange(&health, value) != value && value) {
            ODRelease();
            ODEmit(@"HotkeyPressed");
        }
        ODBounds = CGDisplayBounds(CGMainDisplayID());
        if (!ready || !CGRectEqualToRect(previousBounds, ODBounds)) {
            BOOL wasReady = ready;
            ready = YES;
            previousBounds = ODBounds;
            if (wasReady) {
                ODRelease();
                ODEmit(@"HotkeyPressed");
            }
            ODEmit(@{
                wasReady ? @"OutputsChanged" : @"Ready" : @{
                    @"outputs" : @[ @{
                        @"name" : @"main",
                        @"x" : @(ODBounds.origin.x),
                        @"y" : @(ODBounds.origin.y),
                        @"width" : @(ODBounds.size.width),
                        @"height" : @(ODBounds.size.height)
                    } ]
                }
            });
        }
        if (ODLocked && !hidden) {
            hidden = CGDisplayHideCursor(CGMainDisplayID()) == kCGErrorSuccess;
        }
        if (disconnected && !captureDriftReported) {
            CGEventRef current = CGEventCreate(NULL);
            if (current) {
                CGPoint p = CGEventGetLocation(current);
                if (fabs(p.x - capturePosition.x) > 2 || fabs(p.y - capturePosition.y) > 2) {
                    NSLog(@"OD capture: local cursor drift detected (%.0f, %.0f)",
                          p.x - capturePosition.x, p.y - capturePosition.y);
                    captureDriftReported = YES;
                }
                CFRelease(current);
            }
        }
        if (!ODLocked && hidden) {
            CGDisplayShowCursor(CGMainDisplayID());
            hidden = NO;
        }
        if (value)
            return;
        NSPasteboard *pb = NSPasteboard.generalPasteboard;
        if (pb.changeCount == clipboardCount)
            return;
        clipboardCount = pb.changeCount;
        NSString *text = [pb stringForType:NSPasteboardTypeString];
        NSData *data;
        NSString *mime;
        if (text) {
            data = [text dataUsingEncoding:NSUTF8StringEncoding];
            mime = @"text/plain;charset=utf-8";
        } else {
            data = [pb dataForType:NSPasteboardTypePNG];
            mime = @"image/png";
            if (!data) {
                NSData *tiff = [pb dataForType:NSPasteboardTypeTIFF];
                if (tiff.length && tiff.length <= 10 * 1024 * 1024) {
                    NSBitmapImageRep *rep = [NSBitmapImageRep imageRepWithData:tiff];
                    if (rep.pixelsWide * rep.pixelsHigh <= 25000000)
                        data = [rep representationUsingType:NSBitmapImageFileTypePNG
                                                 properties:@{}];
                }
            }
        }
        if (data.length && data.length <= 10 * 1024 * 1024 && ODClipboard)
            ODClipboard(mime.UTF8String, data.bytes, data.length);
    }
}
