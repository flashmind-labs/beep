// Defers NSApplication termination until Rust finishes its bounded async shutdown
// while the main CFRunLoop stays responsive for WebKit URL scheme tasks.
#import <AppKit/AppKit.h>
#import <CoreFoundation/CoreFoundation.h>
#import <objc/runtime.h>
#import <stdbool.h>
#import <stdio.h>

typedef bool (*BeepShouldTerminateNowFn)(void);
static BeepShouldTerminateNowFn should_terminate_now = NULL;

static NSApplicationTerminateReply beep_should_terminate(
    id self,
    SEL _cmd,
    NSApplication *sender
) {
    (void)self;
    (void)_cmd;
    (void)sender;

    if (!should_terminate_now || should_terminate_now()) {
        return NSTerminateNow;
    }

    return NSTerminateLater;
}

void beep_install_terminate_hook(BeepShouldTerminateNowFn callback) {
    should_terminate_now = callback;
    id delegate = NSApp.delegate;
    if (!delegate || [delegate respondsToSelector:@selector(applicationShouldTerminate:)]) {
        return;
    }

    char types[16];
    snprintf(types, sizeof(types), "%s@:@", @encode(NSApplicationTerminateReply));
    class_addMethod(
        object_getClass(delegate),
        @selector(applicationShouldTerminate:),
        (IMP)beep_should_terminate,
        types
    );
}

void beep_finish_terminate(void) {
    CFRunLoopRef main_loop = CFRunLoopGetMain();
    if (!main_loop) {
        return;
    }

    CFRunLoopPerformBlock(main_loop, kCFRunLoopCommonModes, ^{
        [NSApp replyToApplicationShouldTerminate:YES];
    });
    CFRunLoopWakeUp(main_loop);
}
