#import "native.h"
char *(*ODRequest)(const char *);
void (*ODFree)(char *);
void ODPerform(id request, void (^done)(id)) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:request
                                                   options:NSJSONWritingFragmentsAllowed
                                                     error:nil];
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
      char *raw = ODRequest ? ODRequest(json.UTF8String) : NULL;
      id result =
          raw ? [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:raw]
                                                            dataUsingEncoding:NSUTF8StringEncoding]
                                                options:NSJSONReadingFragmentsAllowed
                                                  error:nil]
              : nil;
      if (raw)
          ODFree(raw);
      dispatch_async(dispatch_get_main_queue(), ^{
        if (done)
            done(result);
      });
    });
}
static void message(NSString *title, NSString *body) {
    NSAlert *alert = [NSAlert new];
    alert.messageText = title;
    alert.informativeText = body ?: @"";
    [NSApp activateIgnoringOtherApps:YES];
    [alert runModal];
}
static NSString *prompt(NSString *title, NSString *body) {
    NSAlert *alert = [NSAlert new];
    alert.messageText = title;
    alert.informativeText = body;
    NSTextField *field = [[NSTextField alloc] initWithFrame:NSMakeRect(0, 0, 320, 24)];
    alert.accessoryView = field;
    [alert addButtonWithTitle:@"Continuar"];
    [alert addButtonWithTitle:@"Cancelar"];
    [NSApp activateIgnoringOtherApps:YES];
    [alert.window setInitialFirstResponder:field];
    return [alert runModal] == NSAlertFirstButtonReturn ? field.stringValue : nil;
}
static NSString *selectItem(NSString *title, NSArray<NSString *> *titles,
                            NSArray<NSString *> *values) {
    if (!titles.count) {
        message(title, @"Nenhum computador disponível.");
        return nil;
    }
    NSAlert *alert = [NSAlert new];
    alert.messageText = title;
    NSPopUpButton *field = [[NSPopUpButton alloc] initWithFrame:NSMakeRect(0, 0, 320, 28)
                                                      pullsDown:NO];
    [field addItemsWithTitles:titles];
    alert.accessoryView = field;
    [alert addButtonWithTitle:@"Continuar"];
    [alert addButtonWithTitle:@"Cancelar"];
    [NSApp activateIgnoringOtherApps:YES];
    return [alert runModal] == NSAlertFirstButtonReturn ? values[field.indexOfSelectedItem] : nil;
}
static void resultMessage(NSString *title, id reply) {
    NSString *text =
        [reply isEqual:@"Ok"]
            ? @"Concluído."
            : ([reply isKindOfClass:NSDictionary.class] ? reply[@"Error"][@"message"] : nil);
    message(title, text ?: @"Não foi possível concluir. Consulte o diagnóstico.");
}
@interface ODApp : NSObject <NSApplicationDelegate>
@property NSStatusItem *item;
@property NSMenuItem *status;
@property NSMenuItem *permissions;
@property NSMenuItem *pause;
@property NSMenuItem *login;
@property NSDictionary *report;
@property BOOL polling;
@end
@implementation ODApp
- (void)organize:(id)sender {
    NSTask *task = [NSTask new];
    task.executableURL = NSBundle.mainBundle.executableURL;
    task.arguments = @[@"gui"];
    [task launchAndReturnError:nil];
}
- (void)add:(NSMenu *)menu title:(NSString *)title action:(SEL)action {
    NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:title action:action keyEquivalent:@""];
    item.target = self;
    [menu addItem:item];
}
- (void)applicationDidFinishLaunching:(NSNotification *)note {
    (void)note;
    ODKeys = [NSMutableSet set];
    ODButtons = [NSMutableSet set];
    self.item = [NSStatusBar.systemStatusBar statusItemWithLength:NSVariableStatusItemLength];
    self.item.button.title = @"OD";
    NSMenu *menu = [NSMenu new];
    self.status = [[NSMenuItem alloc] initWithTitle:@"Iniciando…" action:nil keyEquivalent:@""];
    [menu addItem:self.status];
    self.permissions = [[NSMenuItem alloc] initWithTitle:@"Verificar permissões"
                                                  action:@selector(permissions:)
                                           keyEquivalent:@""];
    self.permissions.target = self;
    [menu addItem:self.permissions];
    [menu addItem:NSMenuItem.separatorItem];
    [self add:menu title:@"Encontrar computadores…" action:@selector(discover:)];
    [self add:menu title:@"Parear com computador…" action:@selector(pair:)];
    [self add:menu title:@"Organizar computadores…" action:@selector(organize:)];
    self.pause = [[NSMenuItem alloc] initWithTitle:@"Pausar"
                                            action:@selector(pause:)
                                     keyEquivalent:@""];
    self.pause.target = self;
    [menu addItem:self.pause];
    [self add:menu title:@"Liberar controle" action:@selector(release:)];
    self.login = [[NSMenuItem alloc] initWithTitle:@"Iniciar ao entrar"
                                            action:@selector(login:)
                                     keyEquivalent:@""];
    self.login.target = self;
    [menu addItem:self.login];
    [self add:menu title:@"Diagnóstico…" action:@selector(doctor:)];
    [menu addItem:NSMenuItem.separatorItem];
    [self add:menu title:@"Sair" action:@selector(quit:)];
    self.item.menu = menu;
    NSNotificationCenter *nc = NSWorkspace.sharedWorkspace.notificationCenter;
    for (NSString *name in @[
             NSWorkspaceWillSleepNotification, NSWorkspaceScreensDidSleepNotification,
             NSWorkspaceSessionDidResignActiveNotification
         ])
        [nc addObserverForName:name
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(NSNotification *n) {
                      (void)n;
                      ODSuspended = YES;
                      ODRelease();
                      ODEmit(@"HotkeyPressed");
                    }];
    for (NSString *name in @[
             NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidWakeNotification,
             NSWorkspaceSessionDidBecomeActiveNotification
         ])
        [nc addObserverForName:name
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(NSNotification *n) {
                      (void)n;
                      ODSuspended = NO;
                    }];
    NSTimer *inputTimer = [NSTimer timerWithTimeInterval:0.1
                                                 repeats:YES
                                                   block:^(NSTimer *timer) {
                                                     (void)timer;
                                                     ODTick();
                                                   }];
    [NSRunLoop.mainRunLoop addTimer:inputTimer forMode:NSRunLoopCommonModes];
    NSTimer *uiTimer = [NSTimer timerWithTimeInterval:1
                                              repeats:YES
                                                block:^(NSTimer *timer) {
                                                  (void)timer;
                                                  [self refresh];
                                                }];
    [NSRunLoop.mainRunLoop addTimer:uiTimer forMode:NSRunLoopCommonModes];
    NSTimer *repeatTimer = [NSTimer timerWithTimeInterval:0.01
                                                  repeats:YES
                                                    block:^(NSTimer *timer) {
                                                      (void)timer;
                                                      ODRepeatTick();
                                                    }];
    [NSRunLoop.mainRunLoop addTimer:repeatTimer forMode:NSRunLoopCommonModes];
    [self refresh];
}
- (void)refresh {
    self.permissions.title = od_health() == 1   ? @"Permissões de entrada pendentes…"
                             : od_health() == 2 ? @"Entrada suspensa: sessão/telas/Secure Input"
                                                : @"Permissões de entrada concedidas";
    self.login.state = SMAppService.mainAppService.status == SMAppServiceStatusEnabled
                           ? NSControlStateValueOn
                           : NSControlStateValueOff;
    if (self.polling)
        return;
    self.polling = YES;
    ODPerform(@"Status", ^(id reply) {
      self.polling = NO;
      NSDictionary *r = [reply isKindOfClass:NSDictionary.class] ? reply[@"Status"] : nil;
      if (!r) {
          self.status.title = @"Motor indisponível — abra diagnóstico";
          return;
      }
      self.report = r;
      NSMutableArray *names = [NSMutableArray array];
      for (NSDictionary *peer in r[@"peers"])
          if ([peer[@"connected"] boolValue])
              [names addObject:peer[@"name"]];
      NSString *pin = [r[@"pending_pin"] isKindOfClass:NSString.class] ? r[@"pending_pin"] : nil;
      self.status.title =
          pin ? [NSString stringWithFormat:@"PIN para o outro computador: %@", pin]
              : [NSString stringWithFormat:@"%@ · %@",
                                           (@{
                                               @"idle" : @"Controle local",
                                               @"controlling" : @"Controlando",
                                               @"controlled" : @"Recebendo controle",
                                               @"requesting" : @"Conectando",
                                               @"pushing" : @"Atravessando",
                                               @"returning" : @"Retornando"
                                           }[r[@"state"]]
                                                ?: r[@"state"]),
                                           [names componentsJoinedByString:@", "]];
      self.pause.title = [r[@"enabled"] boolValue] ? @"Pausar" : @"Retomar";
    });
}
- (void)permissions:(id)sender {
    (void)sender;
    NSString *pane;
    if (!CGPreflightListenEventAccess()) {
        CGRequestListenEventAccess();
        pane = @"Privacy_ListenEvent";
    } else if (!CGPreflightPostEventAccess()) {
        CGRequestPostEventAccess();
        NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt : @YES};
        AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
        pane = @"Privacy_Accessibility";
    } else {
        [self doctor:sender];
        return;
    }
    NSString *url =
        [@"x-apple.systempreferences:com.apple.preference.security?" stringByAppendingString:pane];
    [NSWorkspace.sharedWorkspace openURL:[NSURL URLWithString:url]];
}
- (void)discover:(id)sender {
    (void)sender;
    ODPerform(@"Discover", ^(id r) {
      NSMutableArray *lines = [NSMutableArray array];
      for (NSDictionary *p in r[@"Discovered"])
          [lines addObject:[NSString stringWithFormat:@"%@ · %@", p[@"name"], p[@"address"]]];
      message(
          @"Computadores encontrados",
          lines.count
              ? [lines componentsJoinedByString:@"\n"]
              : @"Nenhum computador encontrado. Inicie o Open Desktop no Linux e confira a LAN.");
    });
}
- (void)pair:(id)sender {
    (void)sender;
    ODPerform(@"Discover", ^(id reply) {
      NSMutableArray *names = [NSMutableArray array];
      for (NSDictionary *peer in reply[@"Discovered"])
          [names addObject:peer[@"name"]];
      NSString *name = selectItem(@"Parear com computador", names, names);
      if (!name)
          return;
      ODPerform(
          @{@"Pair" : @{@"name" : name}},
          ^(id r) {
            if ([r isEqual:@"PinRequired"]) {
                NSString *pin = prompt(@"PIN", @"Digite o PIN exibido no outro computador.");
                if (pin.length)
                    ODPerform(
                        @{@"SubmitPin" : @{@"pin" : pin}},
                        ^(id result) {
                          resultMessage(@"Pareamento", result);
                        });
            } else
                resultMessage(@"Pareamento", r);
          });
    });
}
- (void)edges:(id)sender {
    (void)sender;
    NSMutableArray *names = [NSMutableArray array];
    for (NSDictionary *peer in self.report[@"peers"])
        [names addObject:peer[@"name"]];
    NSString *name = selectItem(@"Configurar computador", names, names);
    if (!name)
        return;
    NSString *side = selectItem(@"Bordas de passagem",
                                @[ @"Todas", @"Esquerda", @"Direita", @"Superior", @"Inferior" ],
                                @[ @"all", @"left", @"right", @"top", @"bottom" ]);
    if (!side)
        return;
    ODPerform(@{@"PeerSet" : @{@"name" : name, @"side" : side}}, ^(id r) {
      resultMessage(@"Bordas", r);
    });
}
- (void)pause:(id)sender {
    (void)sender;
    ODPerform([self.report[@"enabled"] boolValue] ? @"Disable" : @"Enable", ^(id r) {
      (void)r;
      [self refresh];
    });
}
- (void)release:(id)sender {
    (void)sender;
    ODRelease();
    ODPerform(@"Release", nil);
}
- (void)login:(id)sender {
    (void)sender;
    NSError *error = nil;
    if (SMAppService.mainAppService.status == SMAppServiceStatusEnabled)
        [SMAppService.mainAppService unregisterAndReturnError:&error];
    else
        [SMAppService.mainAppService registerAndReturnError:&error];
    if (error)
        message(@"Início automático", error.localizedDescription);
    [self refresh];
}
- (void)doctor:(id)sender {
    (void)sender;
    NSString *details =
        [NSString stringWithFormat:@"Entrada: %@\nUm monitor ativo por computador.\n\n%@\n\nLogs: "
                                   @"~/Library/Logs/Open Desktop/app.log",
                                   self.permissions.title, self.report ?: @{}];
    message(@"Diagnóstico", details);
}
- (void)quit:(id)sender {
    (void)sender;
    ODRelease();
    [NSApp terminate:nil];
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    // Closing the TCP connection releases the peer; never defer termination to IPC.
    (void)sender;
    ODRelease();
    return NSTerminateNow;
}
@end
void od_run(void (*event)(const char *), char *(*request)(const char *), void (*freeReply)(char *),
            void (*clipboard)(const char *, const uint8_t *, size_t)) {
    @autoreleasepool {
        ODEvent = event;
        ODRequest = request;
        ODFree = freeReply;
        ODClipboard = clipboard;
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        ODApp *delegate = [ODApp new];
        NSApp.delegate = delegate;
        [NSApp run];
    }
}

void od_quit(void) {
    CFRunLoopPerformBlock(CFRunLoopGetMain(), kCFRunLoopCommonModes, ^{
      ODRelease();
      [NSApp terminate:nil];
    });
    CFRunLoopWakeUp(CFRunLoopGetMain());
}
