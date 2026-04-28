// Linux-side stub for Apple's <Availability.h>. The vt-ferry
// guest-shim is Linux-only; FFmpeg's videotoolbox.c includes this
// header to gate feature checks via `__builtin_available` and the
// `MAC_OS_X_VERSION_*` macros. We claim the highest OS versions so
// every conditional code path stays enabled — the worker actually
// runs on macOS and supports modern features regardless of what
// the guest-side code thinks it sees.
#ifndef VTF_LINUX_AVAILABILITY_H
#define VTF_LINUX_AVAILABILITY_H

#define __MAC_10_0       1000
#define __MAC_10_4       1040
#define __MAC_10_5       1050
#define __MAC_10_6       1060
#define __MAC_10_7       1070
#define __MAC_10_8       1080
#define __MAC_10_9       1090
#define __MAC_10_10      101000
#define __MAC_10_11      101100
#define __MAC_10_12      101200
#define __MAC_10_13      101300
#define __MAC_10_14      101400
#define __MAC_10_15      101500
#define __MAC_11_0       110000
#define __MAC_12_0       120000
#define __MAC_13_0       130000
#define __MAC_14_0       140000

#ifndef __MAC_OS_X_VERSION_MIN_REQUIRED
#define __MAC_OS_X_VERSION_MIN_REQUIRED __MAC_14_0
#endif
#ifndef __MAC_OS_X_VERSION_MAX_ALLOWED
#define __MAC_OS_X_VERSION_MAX_ALLOWED __MAC_14_0
#endif
#ifndef MAC_OS_X_VERSION_MIN_REQUIRED
#define MAC_OS_X_VERSION_MIN_REQUIRED __MAC_OS_X_VERSION_MIN_REQUIRED
#endif
#ifndef MAC_OS_X_VERSION_MAX_ALLOWED
#define MAC_OS_X_VERSION_MAX_ALLOWED __MAC_OS_X_VERSION_MAX_ALLOWED
#endif

// Define the version macros FFmpeg's videotoolbox.c probes against.
#define MAC_OS_X_VERSION_10_9   __MAC_10_9
#define MAC_OS_VERSION_11_0     __MAC_11_0

// __builtin_available is a clang extension. On Linux GCC, treat
// every probe as "yes, available" so the gated code paths compile
// and run unconditionally.
#ifndef __builtin_available
#define __builtin_available(...) (1)
#endif

// API_AVAILABLE / API_UNAVAILABLE macros are no-ops on Linux —
// availability annotations don't affect codegen here.
#define API_AVAILABLE(...)
#define API_UNAVAILABLE(...)
#define API_DEPRECATED(...)
#define API_DEPRECATED_WITH_REPLACEMENT(...)

#endif // VTF_LINUX_AVAILABILITY_H
