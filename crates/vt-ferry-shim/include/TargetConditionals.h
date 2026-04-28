// Linux-side stub for Apple's <TargetConditionals.h>. We pretend
// to be macOS (TARGET_OS_OSX = 1, TARGET_OS_IPHONE = 0) so FFmpeg's
// videotoolbox.c takes the OSX-only feature paths — the worker
// actually runs on macOS and the guest-shim relays to it.
#ifndef VTF_LINUX_TARGET_CONDITIONALS_H
#define VTF_LINUX_TARGET_CONDITIONALS_H

#define TARGET_OS_OSX            1
#define TARGET_OS_MAC            1
#define TARGET_OS_IPHONE         0
#define TARGET_OS_IOS            0
#define TARGET_OS_TV             0
#define TARGET_OS_WATCH          0
#define TARGET_OS_BRIDGE         0
#define TARGET_OS_SIMULATOR      0
#define TARGET_OS_EMBEDDED       0
#define TARGET_OS_MACCATALYST    0
#define TARGET_OS_DRIVERKIT      0

#define TARGET_CPU_ARM64         1
#define TARGET_CPU_ARM           0
#define TARGET_CPU_X86_64        0
#define TARGET_CPU_X86           0

#endif
