// Loaded into Apple-signed /usr/bin/perl, which MediaRemote still serves (macOS 15.4+ refuses
// unentitled processes). Streams one JSON object per line on stdout whenever the system
// Now Playing state changes; reads one command word per line on stdin. Exits when stdin closes,
// so the helper never outlives notchpad.
#import <AppKit/AppKit.h>
#include <dlfcn.h>

typedef void (^InfoBlock)(NSDictionary *);
static void (*getInfo)(dispatch_queue_t, InfoBlock);
static void (*getPlaying)(dispatch_queue_t, void (^)(BOOL));
static void (*getPID)(dispatch_queue_t, void (^)(int));
static void (*sendCommand)(int, id);
static void (*registerNotifications)(dispatch_queue_t);

static dispatch_queue_t queue;
static NSString *lastLine;

static id value(NSDictionary *info, NSString *key) {
    return info[[@"kMRMediaRemoteNowPlayingInfo" stringByAppendingString:key]];
}

static void emit(NSDictionary *line) {
    NSData *json = [NSJSONSerialization dataWithJSONObject:line options:0 error:nil];
    NSString *text = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
    if ([text isEqualToString:lastLine]) return;
    lastLine = text;
    printf("%s\n", text.UTF8String);
    fflush(stdout);
}

// Three async queries joined into one line; they all run on the serial `queue`.
static void refresh(void) {
    getPID(queue, ^(int pid) {
        getPlaying(queue, ^(BOOL playing) {
            getInfo(queue, ^(NSDictionary *info) {
                NSString *title = value(info, @"Title");
                if (!title) { emit(@{}); return; }
                double elapsed = [value(info, @"ElapsedTime") doubleValue];
                NSDate *stamp = value(info, @"Timestamp");
                // ElapsedTime is as of Timestamp; advance it so the reader needs no clock math.
                if (playing && stamp) elapsed += -stamp.timeIntervalSinceNow;
                NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
                NSData *art = value(info, @"ArtworkData");
                emit(@{
                    @"title": title,
                    @"artist": value(info, @"Artist") ?: @"",
                    @"album": value(info, @"Album") ?: @"",
                    @"duration": @([value(info, @"Duration") doubleValue]),
                    @"position": @(elapsed),
                    @"playing": @(playing),
                    @"bundle": app.bundleIdentifier ?: @"",
                    @"artwork": [art base64EncodedStringWithOptions:0] ?: @"",
                });
            });
        });
    });
}

static void readCommands(void) {
    NSDictionary *codes = @{@"play": @0, @"pause": @1, @"toggle": @2, @"next": @4, @"previous": @5};
    char buffer[64];
    while (fgets(buffer, sizeof buffer, stdin)) {
        NSString *word = [[NSString stringWithUTF8String:buffer] stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
        if (codes[word]) sendCommand([codes[word] intValue], nil);
    }
    _exit(0);
}

void np_run(void) {
    void *lib = dlopen("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote", RTLD_NOW);
    getInfo = dlsym(lib, "MRMediaRemoteGetNowPlayingInfo");
    getPlaying = dlsym(lib, "MRMediaRemoteGetNowPlayingApplicationIsPlaying");
    getPID = dlsym(lib, "MRMediaRemoteGetNowPlayingApplicationPID");
    sendCommand = dlsym(lib, "MRMediaRemoteSendCommand");
    registerNotifications = dlsym(lib, "MRMediaRemoteRegisterForNowPlayingNotifications");
    queue = dispatch_queue_create("notchpad.nowplaying", DISPATCH_QUEUE_SERIAL);

    registerNotifications(queue);
    for (NSString *name in @[@"kMRMediaRemoteNowPlayingInfoDidChangeNotification",
                             @"kMRMediaRemoteNowPlayingApplicationDidChangeNotification",
                             @"kMRMediaRemoteNowPlayingApplicationIsPlayingDidChangeNotification"]) {
        [NSNotificationCenter.defaultCenter addObserverForName:name object:nil queue:nil
                                                    usingBlock:^(NSNotification *n) { dispatch_async(queue, ^{ refresh(); }); }];
    }
    dispatch_async(queue, ^{ refresh(); });
    [NSThread detachNewThreadWithBlock:^{ readCommands(); }];
    CFRunLoopRun();
}
