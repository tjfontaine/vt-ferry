#ifndef VT_FERRY_COREVIDEO_H
#define VT_FERRY_COREVIDEO_H

#include <stddef.h>
#include <stdint.h>

#include "CoreFoundation/CoreFoundation.h"
#include "CoreMedia/CoreMedia.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t CVReturn;
typedef uint32_t CVOptionFlags;

enum {
    kCVReturnSuccess = 0,
    kCVReturnInvalidArgument = -6661,
};

typedef enum CVAttachmentMode {
    kCVAttachmentMode_ShouldNotPropagate = 0,
    kCVAttachmentMode_ShouldPropagate = 1,
} CVAttachmentMode;

typedef enum CVPixelBufferLockFlags {
    kCVPixelBufferLock_ReadOnly = 0x00000001u,
} CVPixelBufferLockFlags;

enum {
    kCVPixelFormatType_32BGRA = 0x42475241u,
    kCVPixelFormatType_420YpCbCr8Planar = 0x79343230u,
    kCVPixelFormatType_420YpCbCr8PlanarFullRange = 0x66343230u,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange = 0x34323076u,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange = 0x34323066u,
    kCVPixelFormatType_422YpCbCr8 = 0x32767579u,
    kCVPixelFormatType_4444AYpCbCr8 = 0x76343038u,
    kCVPixelFormatType_4444AYpCbCr16 = 0x76323136u,
};

typedef struct vtf_cv_buffer *CVBufferRef;
typedef CVBufferRef CVImageBufferRef;
typedef struct vtf_cv_pixel_buffer *CVPixelBufferRef;
typedef struct vtf_cv_pixel_buffer_pool *CVPixelBufferPoolRef;

typedef void *CGColorSpaceRef;

VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferPixelFormatTypeKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferWidthKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferHeightKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferBytesPerRowAlignmentKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferIOSurfacePropertiesKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferOpenGLESCompatibilityKey;
VT_FERRY_EXPORT extern const CFStringRef kCVPixelBufferIOSurfaceOpenGLTextureCompatibilityKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferPixelAspectRatioKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferPixelAspectRatioHorizontalSpacingKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferPixelAspectRatioVerticalSpacingKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferYCbCrMatrixKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferYCbCrMatrix_ITU_R_709_2;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferYCbCrMatrix_ITU_R_601_4;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferYCbCrMatrix_SMPTE_240M_1995;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferYCbCrMatrix_ITU_R_2020;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferColorPrimariesKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferColorPrimaries_ITU_R_709_2;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferColorPrimaries_SMPTE_C;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferColorPrimaries_EBU_3213;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferColorPrimaries_ITU_R_2020;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunctionKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_ITU_R_709_2;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_SMPTE_240M_1995;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_UseGamma;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_ITU_R_2020;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_SMPTE_ST_428_1;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferTransferFunction_ITU_R_2100_HLG;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferGammaLevelKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferCGColorSpaceKey;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_Left;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_Center;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_Top;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_Bottom;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_TopLeft;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocation_BottomLeft;
VT_FERRY_EXPORT extern const CFStringRef kCVImageBufferChromaLocationTopFieldKey;

VT_FERRY_EXPORT CVReturn CVPixelBufferPoolCreate(CFAllocatorRef allocator, CFDictionaryRef poolAttributes, CFDictionaryRef pixelBufferAttributes, CVPixelBufferPoolRef *poolOut);
VT_FERRY_EXPORT CVReturn CVPixelBufferPoolCreatePixelBuffer(CFAllocatorRef allocator, CVPixelBufferPoolRef pixelBufferPool, CVPixelBufferRef *pixelBufferOut);
VT_FERRY_EXPORT void CVPixelBufferPoolRelease(CVPixelBufferPoolRef pool);
VT_FERRY_EXPORT CFDictionaryRef CVPixelBufferPoolGetPixelBufferAttributes(CVPixelBufferPoolRef pool);

VT_FERRY_EXPORT CVPixelBufferRef CVPixelBufferRetain(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT void CVPixelBufferRelease(CVPixelBufferRef pixelBuffer);

VT_FERRY_EXPORT CVReturn CVPixelBufferLockBaseAddress(CVPixelBufferRef pixelBuffer, CVOptionFlags flags);
VT_FERRY_EXPORT CVReturn CVPixelBufferUnlockBaseAddress(CVPixelBufferRef pixelBuffer, CVOptionFlags flags);
VT_FERRY_EXPORT void *CVPixelBufferGetBaseAddress(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT void *CVPixelBufferGetBaseAddressOfPlane(CVPixelBufferRef pixelBuffer, size_t planeIndex);
VT_FERRY_EXPORT size_t CVPixelBufferGetBytesPerRow(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT size_t CVPixelBufferGetBytesPerRowOfPlane(CVPixelBufferRef pixelBuffer, size_t planeIndex);
VT_FERRY_EXPORT size_t CVPixelBufferGetWidth(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT size_t CVPixelBufferGetHeight(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT OSType CVPixelBufferGetPixelFormatType(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT Boolean CVPixelBufferIsPlanar(CVPixelBufferRef pixelBuffer);
VT_FERRY_EXPORT size_t CVPixelBufferGetPlaneCount(CVPixelBufferRef pixelBuffer);

VT_FERRY_EXPORT void CVBufferSetAttachment(CVBufferRef buffer, CFStringRef key, CFTypeRef value, CVAttachmentMode attachmentMode);
VT_FERRY_EXPORT void CVBufferRemoveAttachment(CVBufferRef buffer, CFStringRef key);
VT_FERRY_EXPORT CFDictionaryRef CVBufferGetAttachments(CVBufferRef buffer, CVAttachmentMode attachmentMode);
VT_FERRY_EXPORT CFDictionaryRef CVBufferCopyAttachments(CVBufferRef buffer, CVAttachmentMode attachmentMode);

VT_FERRY_EXPORT CFStringRef CVColorPrimariesGetStringForIntegerCodePoint(int64_t codePoint);
VT_FERRY_EXPORT CFStringRef CVTransferFunctionGetStringForIntegerCodePoint(int64_t codePoint);
VT_FERRY_EXPORT CFStringRef CVYCbCrMatrixGetStringForIntegerCodePoint(int64_t codePoint);
VT_FERRY_EXPORT CGColorSpaceRef CVImageBufferCreateColorSpaceFromAttachments(CFDictionaryRef attachments);

#ifdef __cplusplus
}
#endif

#endif
