#import "native.h"
#include <errno.h>
#include <signal.h>
#include <unistd.h>

static NSString *const restartKey = @"ODPermissionRestartAttempted";
static BOOL probing;
static NSTimeInterval lastProbe;

int od_permission_probe(void) {
    BOOL listen = CGPreflightListenEventAccess();
    BOOL post = CGPreflightPostEventAccess();
    return (listen ? 1 : 0) | (post ? 2 : 0);
}

int od_permission_relaunch(int parent) {
    @autoreleasepool {
        if (parent <= 1)
            return 1;
        for (int attempt = 0; attempt < 100; attempt++) {
            if (kill(parent, 0) == -1 && errno == ESRCH) {
                NSTask *open = [NSTask new];
                open.executableURL = [NSURL fileURLWithPath:@"/usr/bin/open"];
                open.arguments = @[ @"-n", NSBundle.mainBundle.bundlePath ];
                NSError *error;
                if (![open launchAndReturnError:&error])
                    return 1;
                [open waitUntilExit];
                return open.terminationStatus;
            }
            usleep(100000);
        }
        return 1;
    }
}

static void restartAfterApproval(void) {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    if ([defaults boolForKey:restartKey])
        return;
    NSTask *helper = [NSTask new];
    helper.executableURL = NSBundle.mainBundle.executableURL;
    helper.arguments =
        @[ @"--opendesk-permission-relaunch", [NSString stringWithFormat:@"%d", getpid()] ];
    NSError *error;
    if (![helper launchAndReturnError:&error]) {
        NSLog(@"OD permissions: relaunch helper failed: %@", error);
        return;
    }
    [defaults setBool:YES forKey:restartKey];
    [defaults synchronize];
    NSLog(@"OD permissions: fresh process confirmed approval; restarting automatically");
    ODRelease();
    [NSApp terminate:nil];
}

void ODPermissionTick(int health) {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    if (health == 0) {
        if ([defaults boolForKey:restartKey])
            [defaults removeObjectForKey:restartKey];
        return;
    }
    NSTimeInterval now = NSDate.timeIntervalSinceReferenceDate;
    if (health != 1 || probing || now - lastProbe < 2 || [defaults boolForKey:restartKey])
        return;
    lastProbe = now;
    NSTask *probe = [NSTask new];
    probe.executableURL = NSBundle.mainBundle.executableURL;
    probe.arguments = @[ @"--opendesk-permission-probe" ];
    probe.terminationHandler = ^(NSTask *finished) {
      BOOL granted = finished.terminationReason == NSTaskTerminationReasonExit &&
                     finished.terminationStatus == 3;
      dispatch_async(dispatch_get_main_queue(), ^{
        probing = NO;
        if (granted && od_health() == 1)
            restartAfterApproval();
      });
    };
    NSError *error;
    probing = YES;
    if (![probe launchAndReturnError:&error]) {
        probing = NO;
        NSLog(@"OD permissions: probe failed: %@", error);
    } else {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC),
                       dispatch_get_main_queue(), ^{
                         if (probe.running)
                             [probe terminate];
                       });
    }
}
