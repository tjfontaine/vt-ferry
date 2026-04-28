# Local FFmpeg Trace

This document tracks the first locally observed FFmpeg-to-Apple-media ABI evidence available on the current macOS host.

It is not yet the final guest ABI contract. It is evidence gathered from a local FFmpeg 8.1 build with `--enable-videotoolbox`, used to replace speculation with observed symbols and exposed encoder behavior.

## Local Environment

- FFmpeg binary: `/opt/homebrew/bin/ffmpeg`
- FFmpeg version: `8.1`
- Build config includes: `--enable-videotoolbox`

## Local Commands Used

- `ffmpeg -hide_banner -encoders`
- `ffmpeg -hide_banner -h encoder=h264_videotoolbox`
- `ffmpeg -hide_banner -h encoder=hevc_videotoolbox`
- `otool -L /opt/homebrew/bin/ffmpeg`
- `nm -m /opt/homebrew/Cellar/ffmpeg/8.1/lib/libavcodec.62.dylib`
- `strings -a /opt/homebrew/Cellar/ffmpeg/8.1/lib/libavcodec.62.dylib`

## Confirmed Runtime Encoders

Observed locally:

- `h264_videotoolbox`
- `hevc_videotoolbox`
- `prores_videotoolbox`

The local `h264_videotoolbox` encoder advertises these supported pixel formats:

- `videotoolbox_vld`
- `nv12`
- `yuv420p`

## Observed H.264 VideoToolbox Options

Observed in local `ffmpeg -h encoder=h264_videotoolbox` output:

- profile selection
- level selection
- entropy coder selection
- `a53cc`
- `constant_bit_rate`
- `max_slice_bytes`
- `allow_sw`
- `require_sw`
- `realtime`
- `frames_before`
- `frames_after`
- `prio_speed`
- `power_efficient`
- `spatial_aq`
- `max_ref_frames`

This matters because some of these map directly onto VideoToolbox property keys or encoder-specification behavior, while others imply FFmpeg-side option logic that the guest shim must tolerate.

## Observed Imported Apple Symbols

The local Homebrew `libavcodec.62.dylib` imports the following Apple media APIs, which are directly relevant to the guest compatibility target.

### CoreFoundation

- `CFArrayCreate`
- `CFArrayGetCount`
- `CFArrayGetValueAtIndex`
- `CFBooleanGetValue`
- `CFDataCreate`
- `CFDataGetBytes`
- `CFDataGetLength`
- `CFDataGetTypeID`
- `CFDictionaryCreate`
- `CFDictionaryCreateMutable`
- `CFDictionaryGetValueIfPresent`
- `CFDictionarySetValue`
- `CFGetTypeID`
- `CFNumberCreate`
- `CFRelease`
- `CFRetain`
- `CFStringGetCString`
- `CFStringGetLength`
- `CFStringGetMaximumSizeForEncoding`

### CoreMedia

- `CMBlockBufferCopyDataBytes`
- `CMBlockBufferCreateWithMemoryBlock`
- `CMBlockBufferGetDataLength`
- `CMFormatDescriptionGetExtension`
- `CMSampleBufferCreate`
- `CMSampleBufferGetDataBuffer`
- `CMSampleBufferGetDecodeTimeStamp`
- `CMSampleBufferGetFormatDescription`
- `CMSampleBufferGetPresentationTimeStamp`
- `CMSampleBufferGetSampleAttachmentsArray`
- `CMSampleBufferGetTotalSampleSize`
- `CMTimeMake`
- `CMVideoFormatDescriptionCreate`
- `CMVideoFormatDescriptionGetH264ParameterSetAtIndex`

### CoreVideo

- `CVPixelBufferGetBaseAddressOfPlane`
- `CVPixelBufferGetBytesPerRowOfPlane`
- `CVPixelBufferGetHeight`
- `CVPixelBufferGetPixelFormatType`
- `CVPixelBufferGetPlaneCount`
- `CVPixelBufferGetWidth`
- `CVPixelBufferIsPlanar`
- `CVPixelBufferLockBaseAddress`
- `CVPixelBufferPoolCreatePixelBuffer`
- `CVPixelBufferRelease`
- `CVPixelBufferRetain`
- `CVPixelBufferUnlockBaseAddress`

### VideoToolbox

- `VTCompressionSessionCompleteFrames`
- `VTCompressionSessionCreate`
- `VTCompressionSessionEncodeFrame`
- `VTCompressionSessionGetPixelBufferPool`
- `VTCompressionSessionPrepareToEncodeFrames`
- `VTCopySupportedPropertyDictionaryForEncoder`
- `VTDecompressionSessionCreate`
- `VTDecompressionSessionDecodeFrame`
- `VTDecompressionSessionInvalidate`
- `VTDecompressionSessionWaitForAsynchronousFrames`
- `VTSessionCopyProperty`
- `VTSessionSetProperty`

## Observed Imported Constants

The local binary references at least these property keys or related constants:

### CoreVideo / CoreMedia

- `kCVPixelBufferHeightKey`
- `kCVPixelBufferIOSurfaceOpenGLTextureCompatibilityKey`
- `kCVPixelBufferIOSurfacePropertiesKey`
- `kCVPixelBufferPixelFormatTypeKey`
- `kCVPixelBufferWidthKey`
- `kCMSampleAttachmentKey_NotSync`
- `kCMTimeIndefinite`
- `kCMTimeInvalid`

### VideoToolbox

- `kVTCompressionPropertyKey_AllowFrameReordering`
- `kVTCompressionPropertyKey_AverageBitRate`
- `kVTCompressionPropertyKey_ColorPrimaries`
- `kVTCompressionPropertyKey_DataRateLimits`
- `kVTCompressionPropertyKey_MaxH264SliceBytes`
- `kVTCompressionPropertyKey_MaxKeyFrameInterval`
- `kVTCompressionPropertyKey_MoreFramesAfterEnd`
- `kVTCompressionPropertyKey_MoreFramesBeforeStart`
- `kVTCompressionPropertyKey_PixelAspectRatio`
- `kVTCompressionPropertyKey_ProfileLevel`
- `kVTCompressionPropertyKey_Quality`
- `kVTCompressionPropertyKey_TransferFunction`
- `kVTCompressionPropertyKey_YCbCrMatrix`
- `kVTCompressionPropertyKey_H264EntropyMode`
- `kVTCompressionPropertyKey_RealTime`
- `kVTCompressionPropertyKey_TargetQualityForAlpha`
- `kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality`
- `kVTCompressionPropertyKey_ConstantBitRate`
- `kVTCompressionPropertyKey_EncoderID`
- `kVTCompressionPropertyKey_AllowOpenGOP`
- `kVTCompressionPropertyKey_MaximizePowerEfficiency`
- `kVTCompressionPropertyKey_ReferenceBufferCount`
- `kVTCompressionPropertyKey_MaxAllowedFrameQP`
- `kVTCompressionPropertyKey_MinAllowedFrameQP`
- `kVTCompressionPropertyKey_SpatialAdaptiveQPLevel`
- `kVTEncodeFrameOptionKey_ForceKeyFrame`
- `kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder`
- `kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder`
- `kVTVideoEncoderSpecification_EnableLowLatencyRateControl`

## What This Means

- The guest shim target must include more than the minimal `Create/Encode/Complete` path if it aims to support FFmpeg without carrying a larger FFmpeg patch.
- The local FFmpeg binary clearly uses `CVPixelBufferPoolCreatePixelBuffer` and planar `CVPixelBuffer` APIs, which supports designing the guest compatibility layer around pool-backed pixel buffers instead of ad hoc byte-buffer wrappers.
- `VTCopySupportedPropertyDictionaryForEncoder` is useful both for host capability dumping and for shaping protocol negotiation.

## Current Limitation

These observations come from a macOS FFmpeg build, not a Linux guest build of the patched project. They are still valuable because they expose the actual Apple ABI used by FFmpeg's VideoToolbox implementation, but they do not replace tracing the exact guest-target branch once that build exists.

