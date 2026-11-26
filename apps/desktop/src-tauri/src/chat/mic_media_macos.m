// CoreAudio output/input state and MediaRemote pause/play for desktop voice dictation.
// Uses public CoreAudio process/device properties to detect active playback and Bluetooth
// call-mode transitions, and resolves MediaRemote's Play/Pause commands at runtime.
#import <CoreAudio/CoreAudio.h>
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#import <dlfcn.h>
#import <errno.h>
#import <signal.h>
#import <stdbool.h>
#import <stdint.h>
#import <stdlib.h>
#import <string.h>
#import <unistd.h>

#ifndef kAudioHardwarePropertyProcessObjectList
#define kAudioHardwarePropertyProcessObjectList 'prs#'
#endif
#ifndef kAudioProcessPropertyPID
#define kAudioProcessPropertyPID 'ppid'
#endif
#ifndef kAudioProcessPropertyBundleID
#define kAudioProcessPropertyBundleID 'pbid'
#endif
#ifndef kAudioProcessPropertyIsRunningInput
#define kAudioProcessPropertyIsRunningInput 'piri'
#endif
#ifndef kAudioProcessPropertyIsRunningOutput
#define kAudioProcessPropertyIsRunningOutput 'piro'
#endif
#ifndef kAudioDevicePropertyDataSource
#define kAudioDevicePropertyDataSource 'ssrc'
#endif

enum {
    BEEP_OUTPUT_BUILT_IN_SPEAKER = 0,
    BEEP_OUTPUT_EXTERNAL = 1,
    BEEP_OUTPUT_BLUETOOTH = 2,
    BEEP_OUTPUT_UNKNOWN = 3,
};

typedef struct {
    char uid[256];
    uint32_t kind;
    uint32_t sample_rate;
    uint16_t channels;
    bool mic_input_active;
} BeepMacOutputState;

static bool send_media_remote(uint32_t command) {
    typedef Boolean (*SendCommandFn)(uint32_t, CFDictionaryRef);
    static SendCommandFn send_fn = NULL;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        void *handle = dlopen(
            "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote",
            RTLD_LAZY | RTLD_LOCAL
        );
        if (handle) {
            send_fn = (SendCommandFn)dlsym(handle, "MRMediaRemoteSendCommand");
        }
    });
    if (!send_fn) return false;
    return send_fn(command, NULL) != 0;
}

static AudioObjectID default_device(AudioObjectPropertySelector selector) {
    AudioObjectID dev = kAudioObjectUnknown;
    UInt32 size = sizeof(dev);
    AudioObjectPropertyAddress addr = {
        selector,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, &dev) != noErr) {
        return kAudioObjectUnknown;
    }
    return dev;
}

static bool copy_device_uid(AudioObjectID dev, char *buf, size_t cap) {
    if (cap == 0) return false;
    buf[0] = '\0';
    CFStringRef uid = NULL;
    UInt32 size = sizeof(uid);
    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(dev, &addr, 0, NULL, &size, &uid) != noErr || !uid) {
        return false;
    }
    Boolean ok = CFStringGetCString(uid, buf, (CFIndex)cap, kCFStringEncodingUTF8);
    CFRelease(uid);
    return ok != 0;
}

static uint32_t classify_device(AudioObjectID dev, const char *uid) {
    UInt32 transport = 0;
    UInt32 size = sizeof(transport);
    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(dev, &addr, 0, NULL, &size, &transport) != noErr) {
        return BEEP_OUTPUT_UNKNOWN;
    }
    if (transport == kAudioDeviceTransportTypeBluetooth ||
        transport == kAudioDeviceTransportTypeBluetoothLE) {
        return BEEP_OUTPUT_BLUETOOTH;
    }
    if (transport == kAudioDeviceTransportTypeBuiltIn) {
        if (uid && strcasestr(uid, "headphone") != NULL) {
            return BEEP_OUTPUT_EXTERNAL;
        }
        UInt32 source = 0;
        size = sizeof(source);
        AudioObjectPropertyAddress src_addr = {
            kAudioDevicePropertyDataSource,
            kAudioObjectPropertyScopeOutput,
            kAudioObjectPropertyElementMain
        };
        if (AudioObjectGetPropertyData(dev, &src_addr, 0, NULL, &size, &source) == noErr &&
            source == 'hdpn') {
            return BEEP_OUTPUT_EXTERNAL;
        }
        return BEEP_OUTPUT_BUILT_IN_SPEAKER;
    }
    return BEEP_OUTPUT_EXTERNAL;
}

static uint16_t output_channels(AudioObjectID dev) {
    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(dev, &addr, 0, NULL, &size) != noErr ||
        size < sizeof(AudioBufferList)) {
        return 0;
    }
    AudioBufferList *list = (AudioBufferList *)malloc(size);
    if (!list) return 0;
    uint32_t total = 0;
    if (AudioObjectGetPropertyData(dev, &addr, 0, NULL, &size, list) == noErr) {
        for (UInt32 i = 0; i < list->mNumberBuffers; i++) {
            total += list->mBuffers[i].mNumberChannels;
        }
    }
    free(list);
    return total > UINT16_MAX ? UINT16_MAX : (uint16_t)total;
}

static AudioObjectID *copy_process_list(size_t *out_count) {
    *out_count = 0;
    AudioObjectPropertyAddress addr = {
        kAudioHardwarePropertyProcessObjectList,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (!AudioObjectHasProperty(kAudioObjectSystemObject, &addr)) {
        return NULL;
    }
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr, 0, NULL, &size) != noErr ||
        size == 0) {
        return NULL;
    }
    AudioObjectID *procs = (AudioObjectID *)malloc(size);
    if (!procs) return NULL;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, procs) != noErr) {
        free(procs);
        return NULL;
    }
    *out_count = size / sizeof(AudioObjectID);
    return procs;
}

static bool any_input_running(void) {
    size_t count = 0;
    AudioObjectID *procs = copy_process_list(&count);
    if (procs) {
        bool active = false;
        AudioObjectPropertyAddress addr = {
            kAudioProcessPropertyIsRunningInput,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain
        };
        for (size_t i = 0; i < count; i++) {
            UInt32 running = 0;
            UInt32 size = sizeof(running);
            if (AudioObjectGetPropertyData(procs[i], &addr, 0, NULL, &size, &running) == noErr &&
                running != 0) {
                active = true;
                break;
            }
        }
        free(procs);
        return active;
    }

    AudioObjectID in_dev = default_device(kAudioHardwarePropertyDefaultInputDevice);
    if (in_dev == kAudioObjectUnknown) return false;
    UInt32 running = 0;
    UInt32 size = sizeof(running);
    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyDeviceIsRunningSomewhere,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    return AudioObjectGetPropertyData(in_dev, &addr, 0, NULL, &size, &running) == noErr &&
           running != 0;
}

bool beep_macos_output_state(BeepMacOutputState *out) {
    if (!out) return false;
    memset(out, 0, sizeof(*out));

    AudioObjectID dev = default_device(kAudioHardwarePropertyDefaultOutputDevice);
    if (dev == kAudioObjectUnknown) return false;

    copy_device_uid(dev, out->uid, sizeof(out->uid));
    out->kind = classify_device(dev, out->uid);

    Float64 rate = 0.0;
    UInt32 size = sizeof(rate);
    AudioObjectPropertyAddress rate_addr = {
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(dev, &rate_addr, 0, NULL, &size, &rate) == noErr && rate > 0.0) {
        out->sample_rate = (uint32_t)(rate + 0.5);
    }
    out->channels = output_channels(dev);
    out->mic_input_active = any_input_running();
    return true;
}

bool beep_macos_device_present(const char *uid) {
    if (!uid || uid[0] == '\0') return false;
    AudioObjectPropertyAddress addr = {
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr, 0, NULL, &size) != noErr ||
        size == 0) {
        return false;
    }
    AudioObjectID *devs = (AudioObjectID *)malloc(size);
    if (!devs) return false;
    bool found = false;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, devs) == noErr) {
        size_t count = size / sizeof(AudioObjectID);
        for (size_t i = 0; i < count; i++) {
            UInt32 alive = 0;
            UInt32 alive_size = sizeof(alive);
            AudioObjectPropertyAddress alive_addr = {
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
                kAudioObjectPropertyElementMain
            };
            if (AudioObjectGetPropertyData(devs[i], &alive_addr, 0, NULL, &alive_size, &alive) == noErr &&
                alive == 0) {
                continue;
            }
            char candidate[256];
            if (copy_device_uid(devs[i], candidate, sizeof(candidate)) &&
                strcmp(candidate, uid) == 0) {
                found = true;
                break;
            }
        }
    }
    free(devs);
    return found;
}

static bool is_system_sound_daemon(AudioObjectID proc) {
    CFStringRef bundle = NULL;
    UInt32 size = sizeof(bundle);
    AudioObjectPropertyAddress addr = {
        kAudioProcessPropertyBundleID,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(proc, &addr, 0, NULL, &size, &bundle) != noErr || !bundle) {
        return false;
    }
    bool match = CFStringCompare(bundle, CFSTR("com.apple.systemsoundserverd"), 0) == kCFCompareEqualTo;
    CFRelease(bundle);
    return match;
}

bool beep_macos_pause_media(uint32_t *out_procs, size_t cap, size_t *out_len) {
    if (out_len) *out_len = 0;
    size_t count = 0;
    AudioObjectID *procs = copy_process_list(&count);
    if (procs) {
        pid_t self_pid = getpid();
        size_t written = 0;
        AudioObjectPropertyAddress out_addr = {
            kAudioProcessPropertyIsRunningOutput,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain
        };
        AudioObjectPropertyAddress pid_addr = {
            kAudioProcessPropertyPID,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain
        };
        for (size_t i = 0; i < count; i++) {
            UInt32 running = 0;
            UInt32 size = sizeof(running);
            if (AudioObjectGetPropertyData(procs[i], &out_addr, 0, NULL, &size, &running) != noErr ||
                running == 0) {
                continue;
            }
            pid_t pid = 0;
            size = sizeof(pid);
            if (AudioObjectGetPropertyData(procs[i], &pid_addr, 0, NULL, &size, &pid) == noErr &&
                pid == self_pid) {
                continue;
            }
            if (is_system_sound_daemon(procs[i])) continue;
            if (out_procs && written < cap) {
                out_procs[written] = procs[i];
            }
            written++;
        }
        free(procs);
        if (written == 0) return false;
        if (out_len) *out_len = written < cap ? written : cap;
        // kMRPause = 1
        return send_media_remote(1);
    }

    AudioObjectID dev = default_device(kAudioHardwarePropertyDefaultOutputDevice);
    if (dev == kAudioObjectUnknown) return false;
    UInt32 running = 0;
    UInt32 size = sizeof(running);
    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyDeviceIsRunningSomewhere,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    if (AudioObjectGetPropertyData(dev, &addr, 0, NULL, &size, &running) != noErr || running == 0) {
        return false;
    }
    return send_media_remote(1);
}

bool beep_macos_resume_media(const uint32_t *procs, size_t len) {
    if (procs && len > 0) {
        bool any_paused_target = false;
        AudioObjectPropertyAddress pid_addr = {
            kAudioProcessPropertyPID,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain
        };
        AudioObjectPropertyAddress out_addr = {
            kAudioProcessPropertyIsRunningOutput,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain
        };
        for (size_t i = 0; i < len; i++) {
            pid_t pid = 0;
            UInt32 size = sizeof(pid);
            if (AudioObjectGetPropertyData(procs[i], &pid_addr, 0, NULL, &size, &pid) != noErr ||
                pid <= 0) {
                continue;
            }
            if (kill(pid, 0) != 0 && errno != EPERM) {
                continue;
            }
            UInt32 running = 0;
            size = sizeof(running);
            if (AudioObjectGetPropertyData(procs[i], &out_addr, 0, NULL, &size, &running) == noErr &&
                running == 0) {
                any_paused_target = true;
                break;
            }
        }
        if (!any_paused_target) return false;
    }
    // kMRPlay = 0
    return send_media_remote(0);
}

typedef struct {
    AudioObjectID object;
    AudioObjectPropertyAddress addr;
} WatchedProp;

typedef struct {
    dispatch_queue_t queue;
    AudioObjectPropertyListenerBlock block;
    WatchedProp *entries;
    size_t count;
    size_t cap;
} BeepAudioWatcher;

static void watch_add(BeepAudioWatcher *w, AudioObjectID obj, AudioObjectPropertySelector sel, AudioObjectPropertyScope scope) {
    if (obj == kAudioObjectUnknown) return;
    AudioObjectPropertyAddress addr = { sel, scope, kAudioObjectPropertyElementMain };
    if (!AudioObjectHasProperty(obj, &addr)) return;
    if (w->count == w->cap) {
        size_t next_cap = w->cap == 0 ? 16 : w->cap * 2;
        WatchedProp *grown = (WatchedProp *)realloc(w->entries, next_cap * sizeof(WatchedProp));
        if (!grown) return;
        w->entries = grown;
        w->cap = next_cap;
    }
    if (AudioObjectAddPropertyListenerBlock(obj, &addr, w->queue, w->block) == noErr) {
        w->entries[w->count++] = (WatchedProp){ obj, addr };
    }
}

void *beep_macos_watch_start(void (*callback)(void *), void *ctx) {
    if (!callback) return NULL;
    BeepAudioWatcher *w = (BeepAudioWatcher *)calloc(1, sizeof(BeepAudioWatcher));
    if (!w) return NULL;

    w->queue = dispatch_queue_create("com.flashmind.beep.mic-media", DISPATCH_QUEUE_SERIAL);
    w->block = ^(UInt32 count, const AudioObjectPropertyAddress *addresses) {
        (void)count;
        (void)addresses;
        callback(ctx);
    };

    watch_add(w, kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal);
    watch_add(w, kAudioObjectSystemObject, kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal);
    watch_add(w, kAudioObjectSystemObject, kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal);
    watch_add(w, kAudioObjectSystemObject, kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal);

    AudioObjectPropertyAddress devs_addr = {
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &devs_addr, 0, NULL, &size) == noErr &&
        size > 0) {
        AudioObjectID *devs = (AudioObjectID *)malloc(size);
        if (devs) {
            if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &devs_addr, 0, NULL, &size, devs) == noErr) {
                size_t dev_count = size / sizeof(AudioObjectID);
                for (size_t i = 0; i < dev_count; i++) {
                    watch_add(w, devs[i], kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal);
                    watch_add(w, devs[i], kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyScopeOutput);
                    watch_add(w, devs[i], kAudioDevicePropertyDeviceHasChanged, kAudioObjectPropertyScopeGlobal);
                    watch_add(w, devs[i], kAudioDevicePropertyDeviceIsAlive, kAudioObjectPropertyScopeGlobal);
                    watch_add(w, devs[i], kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioObjectPropertyScopeGlobal);
                }
            }
            free(devs);
        }
    }

    size_t proc_count = 0;
    AudioObjectID *procs = copy_process_list(&proc_count);
    if (procs) {
        for (size_t i = 0; i < proc_count; i++) {
            watch_add(w, procs[i], kAudioProcessPropertyIsRunningInput, kAudioObjectPropertyScopeGlobal);
        }
        free(procs);
    }

    return w;
}

void beep_macos_watch_stop(void *raw) {
    if (!raw) return;
    BeepAudioWatcher *w = (BeepAudioWatcher *)raw;
    for (size_t i = 0; i < w->count; i++) {
        AudioObjectRemovePropertyListenerBlock(w->entries[i].object, &w->entries[i].addr, w->queue, w->block);
    }
    // Drain any in-flight callback before returning so `ctx` can be freed safely.
    dispatch_sync(w->queue, ^{});
    free(w->entries);
    w->block = nil;
    w->queue = nil;
    free(w);
}
