#ifndef VT_FERRY_COREFOUNDATION_H
#define VT_FERRY_COREFOUNDATION_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(__GNUC__)
#define VT_FERRY_EXPORT __attribute__((visibility("default")))
#else
#define VT_FERRY_EXPORT
#endif

typedef signed char Boolean;
typedef uint8_t UInt8;
typedef uint32_t UInt32;
typedef int32_t SInt32;
typedef int64_t SInt64;
typedef uint64_t UInt64;
typedef int64_t CFIndex;
typedef uint64_t CFTypeID;
typedef uint32_t CFStringEncoding;
typedef float Float32;

enum {
    kCFStringEncodingUTF8 = 0x08000100u,
};

#ifndef kCFCoreFoundationVersionNumber10_7
#define kCFCoreFoundationVersionNumber10_7 635.0
#endif

typedef struct vtf_cf_object *CFTypeRef;
typedef struct vtf_cf_allocator *CFAllocatorRef;
typedef struct vtf_cf_string *CFStringRef;
typedef struct vtf_cf_number *CFNumberRef;
typedef struct vtf_cf_data *CFDataRef;
typedef struct vtf_cf_array *CFArrayRef;
typedef struct vtf_cf_array *CFMutableArrayRef;
typedef struct vtf_cf_dictionary *CFDictionaryRef;
typedef struct vtf_cf_dictionary *CFMutableDictionaryRef;
typedef struct vtf_cf_boolean *CFBooleanRef;

typedef struct CFRange {
    CFIndex location;
    CFIndex length;
} CFRange;

static inline CFRange CFRangeMake(CFIndex location, CFIndex length) {
    CFRange range = { location, length };
    return range;
}

typedef enum CFNumberType {
    kCFNumberSInt32Type = 3,
    kCFNumberSInt64Type = 4,
    kCFNumberFloat32Type = 5,
    kCFNumberDoubleType = 6,
    kCFNumberIntType = 9,
} CFNumberType;

typedef struct CFArrayCallBacks {
    CFIndex version;
    const void *retain;
    const void *release;
    const void *copyDescription;
    const void *equal;
} CFArrayCallBacks;

typedef struct CFDictionaryKeyCallBacks {
    CFIndex version;
    const void *retain;
    const void *release;
    const void *copyDescription;
    const void *equal;
    const void *hash;
} CFDictionaryKeyCallBacks;

typedef struct CFDictionaryValueCallBacks {
    CFIndex version;
    const void *retain;
    const void *release;
    const void *copyDescription;
    const void *equal;
} CFDictionaryValueCallBacks;

VT_FERRY_EXPORT extern const CFAllocatorRef kCFAllocatorDefault;
VT_FERRY_EXPORT extern const CFAllocatorRef kCFAllocatorNull;
VT_FERRY_EXPORT extern const CFBooleanRef kCFBooleanTrue;
VT_FERRY_EXPORT extern const CFBooleanRef kCFBooleanFalse;
VT_FERRY_EXPORT extern const CFArrayCallBacks kCFTypeArrayCallBacks;
VT_FERRY_EXPORT extern const CFDictionaryKeyCallBacks kCFTypeDictionaryKeyCallBacks;
VT_FERRY_EXPORT extern const CFDictionaryValueCallBacks kCFTypeDictionaryValueCallBacks;
VT_FERRY_EXPORT extern const CFDictionaryKeyCallBacks kCFCopyStringDictionaryKeyCallBacks;

VT_FERRY_EXPORT CFStringRef VTF_CFStringCreateStatic(const char *bytes);
#define CFSTR(bytes_literal) VTF_CFStringCreateStatic(bytes_literal)

VT_FERRY_EXPORT CFTypeRef CFRetain(CFTypeRef value);
VT_FERRY_EXPORT void CFRelease(CFTypeRef value);
VT_FERRY_EXPORT CFTypeID CFGetTypeID(CFTypeRef value);
VT_FERRY_EXPORT Boolean CFEqual(CFTypeRef lhs, CFTypeRef rhs);

VT_FERRY_EXPORT CFIndex CFStringGetLength(CFStringRef value);
VT_FERRY_EXPORT CFIndex CFStringGetMaximumSizeForEncoding(CFIndex length, CFStringEncoding encoding);
VT_FERRY_EXPORT Boolean CFStringGetCString(CFStringRef value, char *buffer, CFIndex buffer_size, CFStringEncoding encoding);

VT_FERRY_EXPORT CFNumberRef CFNumberCreate(CFAllocatorRef allocator, CFNumberType number_type, const void *value_ptr);
VT_FERRY_EXPORT CFTypeID CFDataGetTypeID(void);
VT_FERRY_EXPORT CFDataRef CFDataCreate(CFAllocatorRef allocator, const UInt8 *bytes, CFIndex length);
VT_FERRY_EXPORT CFIndex CFDataGetLength(CFDataRef data);
VT_FERRY_EXPORT void CFDataGetBytes(CFDataRef data, CFRange range, UInt8 *buffer);

VT_FERRY_EXPORT CFArrayRef CFArrayCreate(CFAllocatorRef allocator, const void **values, CFIndex num_values, const CFArrayCallBacks *callbacks);
VT_FERRY_EXPORT CFIndex CFArrayGetCount(CFArrayRef array);
VT_FERRY_EXPORT const void *CFArrayGetValueAtIndex(CFArrayRef array, CFIndex index);

VT_FERRY_EXPORT CFDictionaryRef CFDictionaryCreate(CFAllocatorRef allocator, const void **keys, const void **values, CFIndex num_values, const CFDictionaryKeyCallBacks *key_callbacks, const CFDictionaryValueCallBacks *value_callbacks);
VT_FERRY_EXPORT CFMutableDictionaryRef CFDictionaryCreateMutable(CFAllocatorRef allocator, CFIndex capacity, const CFDictionaryKeyCallBacks *key_callbacks, const CFDictionaryValueCallBacks *value_callbacks);
VT_FERRY_EXPORT void CFDictionarySetValue(CFMutableDictionaryRef dictionary, const void *key, const void *value);
VT_FERRY_EXPORT const void *CFDictionaryGetValue(CFDictionaryRef dictionary, const void *key);
VT_FERRY_EXPORT Boolean CFDictionaryGetValueIfPresent(CFDictionaryRef dictionary, const void *key, const void **value);
VT_FERRY_EXPORT Boolean CFDictionaryContainsKey(CFDictionaryRef dictionary, const void *key);
VT_FERRY_EXPORT CFDictionaryRef CFDictionaryCreateCopy(CFAllocatorRef allocator, CFDictionaryRef dictionary);

VT_FERRY_EXPORT Boolean CFBooleanGetValue(CFBooleanRef boolean_value);

#ifdef __cplusplus
}
#endif

#endif
