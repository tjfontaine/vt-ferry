#ifndef VT_FERRY_COREMEDIA_H
#define VT_FERRY_COREMEDIA_H

#include <stddef.h>
#include <stdint.h>

#include "CoreFoundation/CoreFoundation.h"

#ifdef __cplusplus
extern "C" {
#endif

#ifndef VTF_OSSTATUS_DEFINED
#define VTF_OSSTATUS_DEFINED 1
typedef int32_t OSStatus;
#endif
typedef uint32_t OSType;
typedef uint32_t FourCharCode;
typedef int64_t CMItemCount;
typedef uint32_t CMBlockBufferFlags;
typedef OSType CMVideoCodecType;

enum {
    noErr = 0,
    kCMBlockBufferNoErr = 0,
    kCMVideoCodecType_H264 = 0x61766331u,    // 'avc1'
    kCMFormatDescriptionError_InvalidParameter = -12712,
    // CMSampleBuffer error codes used by FFmpeg's videotoolbox
    // hwaccel — values mirror Apple's CMSampleBuffer.h.
    kCMSampleBufferError_RequiredParameterMissing = -12731,
};

// Codec FourCCs FFmpeg's videotoolbox.c probes against. Only the
// older codecs (MPEG1/2/4 video, H.263) get defined here —
// FFmpeg's own videotoolbox.c / videotoolboxenc.c have
// `#ifndef`-guarded fallbacks for the newer codecs (HEVC, VP9,
// AV1, HEVCWithAlpha), so re-defining them on our side would
// double-define and the build collides.
//
// FFmpeg compiles enum entries that name these constants, e.g.
//   enum { ... kCMVideoCodecType_HEVC = 0x68766331, ... };
// — using `#define` would expand the name to a literal before
// the enum parser saw it, producing invalid syntax. The newer
// codecs land their fallbacks INSIDE the same translation
// units that need them, so we don't need to provide them.
#define kCMVideoCodecType_H263          0x68323633u  /* 'h263' */
#define kCMVideoCodecType_MPEG1Video    0x6D703176u  /* 'mp1v' */
#define kCMVideoCodecType_MPEG2Video    0x6D703276u  /* 'mp2v' */
#define kCMVideoCodecType_MPEG4Video    0x6D703476u  /* 'mp4v' */

// stdbool-style aliases for older ObjC code paths that haven't
// migrated to <stdbool.h>. Apple's MacTypes.h defines these.
#ifndef TRUE
#define TRUE 1
#endif
#ifndef FALSE
#define FALSE 0
#endif

typedef struct CMTime {
    int64_t value;
    int32_t timescale;
    uint32_t flags;
    int64_t epoch;
} CMTime;

enum {
    kCMTimeFlags_Valid = 1u << 0,
    kCMTimeFlags_HasBeenRounded = 1u << 1,
    kCMTimeFlags_PositiveInfinity = 1u << 2,
    kCMTimeFlags_NegativeInfinity = 1u << 3,
    kCMTimeFlags_Indefinite = 1u << 4,
};

#define CMTIME_IS_INVALID(time_value) (((time_value).flags & kCMTimeFlags_Valid) == 0)

typedef struct vtf_cm_block_buffer *CMBlockBufferRef;
typedef struct vtf_cm_format_description *CMFormatDescriptionRef;
typedef CMFormatDescriptionRef CMVideoFormatDescriptionRef;
typedef struct vtf_cm_sample_buffer *CMSampleBufferRef;

typedef struct CMSampleTimingInfo {
    CMTime duration;
    CMTime presentationTimeStamp;
    CMTime decodeTimeStamp;
} CMSampleTimingInfo;

typedef OSStatus (*CMSampleBufferMakeDataReadyCallback)(CMSampleBufferRef sample_buffer, void *refcon);

VT_FERRY_EXPORT extern const CMTime kCMTimeInvalid;
VT_FERRY_EXPORT extern const CMTime kCMTimeIndefinite;
VT_FERRY_EXPORT extern const CFStringRef kCMSampleAttachmentKey_NotSync;
VT_FERRY_EXPORT extern const CFStringRef kCMFormatDescriptionKey_PixelAspectRatioHorizontalSpacing;
VT_FERRY_EXPORT extern const CFStringRef kCMFormatDescriptionKey_PixelAspectRatioVerticalSpacing;
VT_FERRY_EXPORT extern const CFStringRef kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms;
VT_FERRY_EXPORT extern const CFStringRef kCMFormatDescriptionExtension_VerbatimSampleDescription;

VT_FERRY_EXPORT CMTime CMTimeMake(int64_t value, int32_t timescale);
VT_FERRY_EXPORT int32_t CMTimeCompare(CMTime lhs, CMTime rhs);

VT_FERRY_EXPORT OSStatus CMBlockBufferCreateWithMemoryBlock(
    CFAllocatorRef structureAllocator,
    void *memoryBlock,
    size_t blockLength,
    CFAllocatorRef blockAllocator,
    const void *customBlockSource,
    size_t offsetToData,
    size_t dataLength,
    CMBlockBufferFlags flags,
    CMBlockBufferRef *blockBufferOut
);

VT_FERRY_EXPORT size_t CMBlockBufferGetDataLength(CMBlockBufferRef buffer);
VT_FERRY_EXPORT OSStatus CMBlockBufferCopyDataBytes(CMBlockBufferRef buffer, size_t offsetToData, size_t dataLength, void *destination);
VT_FERRY_EXPORT OSStatus CMBlockBufferGetDataPointer(CMBlockBufferRef buffer, size_t offset, size_t *lengthAtOffsetOut, size_t *totalLengthOut, char **dataPointerOut);

VT_FERRY_EXPORT OSStatus CMSampleBufferCreate(
    CFAllocatorRef allocator,
    CMBlockBufferRef dataBuffer,
    Boolean dataReady,
    CMSampleBufferMakeDataReadyCallback makeDataReadyCallback,
    void *makeDataReadyRefcon,
    CMFormatDescriptionRef formatDescription,
    CMItemCount numSamples,
    CMItemCount numSampleTimingEntries,
    const CMSampleTimingInfo *sampleTimingArray,
    CMItemCount numSampleSizeEntries,
    const size_t *sampleSizeArray,
    CMSampleBufferRef *sampleBufferOut
);

VT_FERRY_EXPORT CMBlockBufferRef CMSampleBufferGetDataBuffer(CMSampleBufferRef sampleBuffer);
VT_FERRY_EXPORT CMFormatDescriptionRef CMSampleBufferGetFormatDescription(CMSampleBufferRef sampleBuffer);
VT_FERRY_EXPORT CMTime CMSampleBufferGetPresentationTimeStamp(CMSampleBufferRef sampleBuffer);
VT_FERRY_EXPORT CMTime CMSampleBufferGetDecodeTimeStamp(CMSampleBufferRef sampleBuffer);
VT_FERRY_EXPORT CFArrayRef CMSampleBufferGetSampleAttachmentsArray(CMSampleBufferRef sampleBuffer, Boolean createIfNecessary);
VT_FERRY_EXPORT size_t CMSampleBufferGetTotalSampleSize(CMSampleBufferRef sampleBuffer);

VT_FERRY_EXPORT OSStatus CMVideoFormatDescriptionCreate(
    CFAllocatorRef allocator,
    CMVideoCodecType codecType,
    int32_t width,
    int32_t height,
    CFDictionaryRef extensions,
    CMFormatDescriptionRef *formatDescriptionOut
);

VT_FERRY_EXPORT CFTypeRef CMFormatDescriptionGetExtension(CMFormatDescriptionRef formatDescription, CFStringRef extensionKey);
VT_FERRY_EXPORT OSStatus CMVideoFormatDescriptionGetH264ParameterSetAtIndex(CMFormatDescriptionRef videoDesc, size_t parameterSetIndex, const uint8_t **parameterSetPointerOut, size_t *parameterSetSizeOut, size_t *parameterSetCountOut, int32_t *NALUnitHeaderLengthOut);
VT_FERRY_EXPORT OSStatus CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(CMFormatDescriptionRef videoDesc, size_t parameterSetIndex, const uint8_t **parameterSetPointerOut, size_t *parameterSetSizeOut, size_t *parameterSetCountOut, int32_t *NALUnitHeaderLengthOut);

#ifdef __cplusplus
}
#endif

#endif
