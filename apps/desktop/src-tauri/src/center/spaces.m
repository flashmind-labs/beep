// WindowServer identifies a Space by a number for this login and a UUID on disk.
// Resolve its optional placement APIs at runtime so a missing API leaves the window usable.
#import <AppKit/AppKit.h>
#import <dlfcn.h>
#import <stdbool.h>

static struct {
    int (*connection)(void);
    CFArrayRef (*displays)(int);
    CFArrayRef (*windowSpaces)(int, int, CFArrayRef);
    void (*move)(int, CFArrayRef, uint64_t);
} api;

static bool available(void) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        void *lib = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY | RTLD_LOCAL);
        if (!lib) return;
        api.connection = dlsym(lib, "SLSMainConnectionID");
        api.displays = dlsym(lib, "SLSCopyManagedDisplaySpaces");
        api.windowSpaces = dlsym(lib, "SLSCopySpacesForWindows");
        api.move = dlsym(lib, "SLSMoveWindowsToManagedSpace");
    });
    return api.connection && api.displays && api.windowSpaces;
}

static NSArray *spacesFor(int cid, NSArray *windows) {
    return CFBridgingRelease(api.windowSpaces(cid, 7, (__bridge CFArrayRef)windows));
}

static NSArray *userSpaces(int cid) {
    NSArray *displays = CFBridgingRelease(api.displays(cid));
    NSMutableArray *spaces = [NSMutableArray array];
    for (NSDictionary *display in displays) {
        for (NSDictionary *space in display[@"Spaces"]) {
            if ([space[@"type"] isEqual:@0] && [space[@"uuid"] isKindOfClass:NSString.class] &&
                [space[@"ManagedSpaceID"] isKindOfClass:NSNumber.class]) [spaces addObject:space];
        }
    }
    return spaces;
}

// WindowServer uses an empty UUID for the original system Space.
static NSUUID *identifier(NSDictionary *space) {
    NSString *value = space[@"uuid"];
    return [[NSUUID alloc] initWithUUIDString:value.length ? value : @"00000000-0000-0000-0000-000000000000"];
}

bool beep_window_space(NSWindow *window, unsigned char *bytes) {
    if (!available()) return false;
    int cid = api.connection();
    NSArray *ids = spacesFor(cid, @[@(window.windowNumber)]);
    for (NSDictionary *space in userSpaces(cid)) {
        if (![ids containsObject:space[@"ManagedSpaceID"]]) continue;
        NSUUID *uuid = identifier(space);
        if (!uuid) continue;
        [uuid getUUIDBytes:bytes];
        return true;
    }
    return false;
}

void beep_place_window(NSWindow *window, const unsigned char *bytes) {
    if (!available() || !api.move) return;
    int cid = api.connection();
    NSUUID *uuid = [[NSUUID alloc] initWithUUIDBytes:bytes];
    NSArray *spaces = userSpaces(cid);
    NSDictionary *target = spaces.firstObject;
    for (NSDictionary *space in spaces) {
        if ([identifier(space) isEqual:uuid]) {
            target = space;
            break;
        }
    }
    if (!target) return;
    uint32_t wid = (uint32_t)window.windowNumber;
    NSArray *windows = @[@(wid)];
    NSNumber *sid = target[@"ManagedSpaceID"];
    if ([spacesFor(cid, windows) containsObject:sid]) return;

    api.move(cid, (__bridge CFArrayRef)windows, sid.unsignedLongLongValue);
}
