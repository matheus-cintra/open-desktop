#import "../crates/opendesk-macos/native/native.h"
#import <IOKit/hidsystem/IOLLEvent.h>
#include <assert.h>
int main(void) {
    @autoreleasepool {
        for (int key = 0; key < 127; key++) {
            int code = ODEvdev(key);
            if (code >= 0)
                assert(ODVirtualKey(code) == key);
        }
        assert(ODVirtualKey(9999) == -1);
        assert(ODEvdev(9999) == -1);
        assert(ODVirtualKey(29) == 59);
        assert(ODEvdev(55) == 125);
        // A release of one side must not release (or press) the other side.
        const struct {
            int left, right;
            uint64_t generic, lflag, rflag;
        } modifiers[] = {
            {59, 62, kCGEventFlagMaskControl, NX_DEVICELCTLKEYMASK, NX_DEVICERCTLKEYMASK},
            {56, 60, kCGEventFlagMaskShift, NX_DEVICELSHIFTKEYMASK, NX_DEVICERSHIFTKEYMASK},
            {55, 54, kCGEventFlagMaskCommand, NX_DEVICELCMDKEYMASK, NX_DEVICERCMDKEYMASK},
            {58, 61, kCGEventFlagMaskAlternate, NX_DEVICELALTKEYMASK, NX_DEVICERALTKEYMASK},
        };
        for (size_t i = 0; i < sizeof(modifiers) / sizeof(modifiers[0]); i++) {
            int l = modifiers[i].left, r = modifiers[i].right;
            uint64_t g = modifiers[i].generic, lf = modifiers[i].lflag, rf = modifiers[i].rflag;
            assert(ODModifierPressed(l, g | lf));
            assert(!ODModifierPressed(r, g | lf));
            assert(ODModifierPressed(l, g | lf | rf));
            assert(ODModifierPressed(r, g | lf | rf));
            assert(!ODModifierPressed(l, g | rf));
            assert(ODModifierPressed(r, g | rf));
            assert(!ODModifierPressed(l, 0) && !ODModifierPressed(r, 0));
            assert(ODModifierPressed(l, g));
        }
        CGRect bounds = CGRectMake(-1728, -100, 1728, 1117);
        CGPoint p = ODClamp(CGPointMake(-9999, 9999), bounds);
        assert(p.x == -1728 && p.y == 1016);
        assert([ODSideAt(CGPointMake(-1728, 0), -1, 0, bounds) isEqual:@"Left"]);
        assert([ODSideAt(CGPointMake(-1, 0), 1, 0, bounds) isEqual:@"Right"]);
        assert([ODSideAt(CGPointMake(-500, -100), 0, -1, bounds) isEqual:@"Top"]);
        assert([ODSideAt(CGPointMake(-500, 1016), 0, 1, bounds) isEqual:@"Bottom"]);
        assert(ODSideAt(CGPointMake(-1728, 0), 1, 0, bounds) == nil);
        assert(ODSideAt(CGPointMake(-500, 500), 0, 1, bounds) == nil);
        puts("Native key mapping, logical geometry and four outward edges passed (no input "
             "captured or injected).");
    }
    return 0;
}
