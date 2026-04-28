#ifndef VT_FERRY_VIDEOTOOLBOX_H
#define VT_FERRY_VIDEOTOOLBOX_H

#include <stddef.h>
#include <stdint.h>

#include "CoreFoundation/CoreFoundation.h"
#include "CoreMedia/CoreMedia.h"
#include "CoreVideo/CoreVideo.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct vtf_vt_session *VTSessionRef;
typedef VTSessionRef VTCompressionSessionRef;
typedef struct vtf_vt_decompression_session *VTDecompressionSessionRef;

typedef uint32_t VTEncodeInfoFlags;
typedef uint32_t VTDecodeInfoFlags;

enum {
    kVTEncodeInfo_Asynchronous = 1u << 0,
    kVTEncodeInfo_FrameDropped = 1u << 1,
};

enum {
    kVTPropertyNotSupportedErr = -12900,
    kVTPropertyReadOnlyErr = -12901,
    kVTParameterErr = -12902,
    kVTInvalidSessionErr = -12903,
    kVTAllocationFailedErr = -12904,
    kVTCouldNotFindVideoEncoderErr = -12908,
    // Decode-side error codes mirrored from Apple's VTErrors.h.
    // FFmpeg's videotoolbox hwaccel maps each to a libavcodec
    // error category; the values here just need to match the
    // canonical OSStatus integers so guest <-> host agree on
    // what error came back from VTDecompressionSession.
    kVTVideoDecoderBadDataErr = -12909,
    kVTVideoDecoderUnsupportedDataFormatErr = -12910,
    kVTVideoDecoderMalfunctionErr = -12911,
    kVTVideoEncoderMalfunctionErr = -12912,
    kVTVideoDecoderNotAvailableNowErr = -12913,
    kVTVideoEncoderNotAvailableNowErr = -12915,
    kVTCouldNotFindVideoDecoderErr = -12916,
};

typedef void (*VTCompressionOutputCallback)(
    void *outputCallbackRefCon,
    void *sourceFrameRefCon,
    OSStatus status,
    VTEncodeInfoFlags infoFlags,
    CMSampleBufferRef sampleBuffer
);

VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_RealTime;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_ProfileLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_AverageBitRate;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_ColorPrimaries;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_ExpectedFrameRate;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MaxKeyFrameInterval;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MoreFramesAfterEnd;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MoreFramesBeforeStart;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_AllowFrameReordering;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_DataRateLimits;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_H264EntropyMode;
VT_FERRY_EXPORT extern const CFStringRef kVTH264EntropyMode_CABAC;
VT_FERRY_EXPORT extern const CFStringRef kVTH264EntropyMode_CAVLC;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_PixelAspectRatio;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_Quality;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_TransferFunction;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_YCbCrMatrix;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_EncoderID;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MaxH264SliceBytes;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_ConstantBitRate;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_TargetQualityForAlpha;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_AllowOpenGOP;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MaximizePowerEfficiency;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_SpatialAdaptiveQPLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_ReferenceBufferCount;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MaxAllowedFrameQP;
VT_FERRY_EXPORT extern const CFStringRef kVTCompressionPropertyKey_MinAllowedFrameQP;
VT_FERRY_EXPORT extern const CFStringRef kVTVideoEncoderSpecification_EncoderID;
VT_FERRY_EXPORT extern const CFStringRef kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder;
VT_FERRY_EXPORT extern const CFStringRef kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder;
VT_FERRY_EXPORT extern const CFStringRef kVTVideoEncoderSpecification_EnableLowLatencyRateControl;
VT_FERRY_EXPORT extern const CFStringRef kVTEncodeFrameOptionKey_ForceKeyFrame;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_1_3;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_3_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_3_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_3_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_4_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_4_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_4_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_5_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_5_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_5_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Baseline_AutoLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_3_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_3_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_3_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_4_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_4_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_4_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_5_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_5_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_5_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Main_AutoLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_3_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_3_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_3_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_4_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_4_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_4_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_5_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_5_1;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_5_2;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_High_AutoLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Extended_5_0;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_Extended_AutoLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel;
VT_FERRY_EXPORT extern const CFStringRef kVTProfileLevel_H264_ConstrainedHigh_AutoLevel;

VT_FERRY_EXPORT OSStatus VTCompressionSessionCreate(
    CFAllocatorRef allocator,
    int32_t width,
    int32_t height,
    CMVideoCodecType codecType,
    CFDictionaryRef encoderSpecification,
    CFDictionaryRef sourceImageBufferAttributes,
    CFAllocatorRef compressedDataAllocator,
    VTCompressionOutputCallback outputCallback,
    void *outputCallbackRefCon,
    VTCompressionSessionRef *compressionSessionOut
);

VT_FERRY_EXPORT void VTCompressionSessionInvalidate(VTCompressionSessionRef session);
VT_FERRY_EXPORT CVPixelBufferPoolRef VTCompressionSessionGetPixelBufferPool(VTCompressionSessionRef session);
VT_FERRY_EXPORT OSStatus VTCompressionSessionPrepareToEncodeFrames(VTCompressionSessionRef session);
VT_FERRY_EXPORT OSStatus VTCompressionSessionEncodeFrame(VTCompressionSessionRef session, CVImageBufferRef imageBuffer, CMTime presentationTimeStamp, CMTime duration, CFDictionaryRef frameProperties, void *sourceFrameRefcon, VTEncodeInfoFlags *infoFlagsOut);
VT_FERRY_EXPORT OSStatus VTCompressionSessionCompleteFrames(VTCompressionSessionRef session, CMTime completeUntilPresentationTimeStamp);

VT_FERRY_EXPORT OSStatus VTSessionSetProperty(VTSessionRef session, CFStringRef propertyKey, CFTypeRef propertyValue);
VT_FERRY_EXPORT OSStatus VTSessionCopyProperty(VTSessionRef session, CFStringRef propertyKey, CFAllocatorRef allocator, void *propertyValueOut);
VT_FERRY_EXPORT OSStatus VTCopySupportedPropertyDictionaryForEncoder(int32_t width, int32_t height, CMVideoCodecType codecType, CFDictionaryRef encoderSpecification, CFStringRef *encoderIDOut, CFDictionaryRef *supportedPropertiesOut);

// VTDecompressionSession surface — Phase 10 decode bring-up.
// VTDecompressionSessionRef + VTDecodeInfoFlags are typedef'd
// near the top of this header alongside VTSessionRef.
typedef uint32_t VTDecodeFrameFlags;

typedef void (*VTDecompressionOutputCallback)(
    void *decompressionOutputRefCon,
    void *sourceFrameRefCon,
    OSStatus status,
    VTDecodeInfoFlags infoFlags,
    CVImageBufferRef imageBuffer,
    CMTime presentationTimeStamp,
    CMTime presentationDuration
);

typedef struct {
    VTDecompressionOutputCallback decompressionOutputCallback;
    void *decompressionOutputRefCon;
} VTDecompressionOutputCallbackRecord;

VT_FERRY_EXPORT OSStatus VTDecompressionSessionCreate(
    CFAllocatorRef allocator,
    CMVideoFormatDescriptionRef videoFormatDescription,
    CFDictionaryRef videoDecoderSpecification,
    CFDictionaryRef destinationImageBufferAttributes,
    const VTDecompressionOutputCallbackRecord *outputCallback,
    VTDecompressionSessionRef *decompressionSessionOut
);

VT_FERRY_EXPORT void VTDecompressionSessionInvalidate(VTDecompressionSessionRef session);
VT_FERRY_EXPORT OSStatus VTDecompressionSessionDecodeFrame(
    VTDecompressionSessionRef session,
    CMSampleBufferRef sampleBuffer,
    VTDecodeFrameFlags decodeFlags,
    void *sourceFrameRefCon,
    VTDecodeInfoFlags *infoFlagsOut
);
VT_FERRY_EXPORT OSStatus VTDecompressionSessionWaitForAsynchronousFrames(VTDecompressionSessionRef session);
VT_FERRY_EXPORT OSStatus VTDecompressionSessionFinishDelayedFrames(VTDecompressionSessionRef session);

#ifdef __cplusplus
}
#endif

#endif
