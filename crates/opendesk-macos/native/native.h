#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ServiceManagement/ServiceManagement.h>
#include <stdatomic.h>
extern void (*ODEvent)(const char *);
extern void (*ODClipboard)(const char *, const uint8_t *, size_t);
extern char *(*ODRequest)(const char *);
extern void (*ODFree)(char *);
extern BOOL ODGrab, ODLocked, ODSuspended;
extern CFMachPortRef ODTap;
extern CGRect ODBounds;
extern NSArray *ODStrips;
extern NSString *ODEdge;
extern NSPanel *ODBar;
extern NSColor *ODColor;
extern uint64_t ODFlags;
extern NSMutableSet *ODKeys, *ODButtons;
extern NSDictionary *ODHotkey;
extern const int64_t ODMarker;
void ODEmit(id value);
void ODRelease(void);
void ODEdges(CGPoint point, double dx, double dy);
void ODTick(void);
void ODBackendStarted(void);
void ODPost(CGEventRef event);
void ODHandle(id command);
int ODVirtualKey(int evdev);
int ODEvdev(int virtualKey);
BOOL ODModifierPressed(int virtualKey, uint64_t flags);
void ODPerform(id request, void (^done)(id));

int od_health(void);

CGPoint ODClamp(CGPoint point, CGRect bounds);
NSString *ODSideAt(CGPoint point, double dx, double dy, CGRect bounds);
void ODRepeatTick(void);
void ODRepeatCancel(void);

BOOL ODCapturePointer(void);

int od_permission_probe(void);
int od_permission_relaunch(int parent);
void ODPermissionTick(int health);
