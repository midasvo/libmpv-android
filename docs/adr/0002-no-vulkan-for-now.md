# ADR 0002: Keep Vulkan disabled for now

- **Status:** accepted
- **Date:** 2026-11-14
- **Context links:** #26 (the evaluation), upstream's unmerged `vulkan` branch (cdd016e, 5915f1c)

## Context

The build switches Vulkan off: `-Dvulkan=disabled` in `libplacebo.sh`, `--disable-vulkan` in
`ffmpeg.sh`, and `shaderc.sh` is not in the dependency tree. So `vo=gpu-next` renders through
libplacebo's OpenGL ES backend. #26 asked whether Ephyra needs Vulkan, and set one condition: without
a concrete feature that OpenGL ES can't deliver, it isn't worth doing.

How Ephyra drives mpv today (`MPVPlayer.kt`): `vo=gpu-next`, `gpu-context=android`, `opengl-es=yes`,
`hwdec=mediacodec`. mpv is the backend people switch to for what ExoPlayer can't play: Dolby Vision
profiles, ASS subtitles, odd audio. ExoPlayer stays the default backend.

What the sources say (mpv 0.42.0, libplacebo 7.387.0, NDK 30.0.16248370):

- **Vulkan would turn off hardware decoding.** With `hwdec=mediacodec` and `vo=gpu-next`, decoded
  frames reach the renderer through the `aimagereader` interop, without a copy. It is the only interop
  for MediaCodec frames, and it refuses anything but OpenGL (`video/out/hwdec/hwdec_aimagereader.c`,
  `if (!ra_is_gl(...))`). mpv has no Vulkan import for Android hardware buffers. On
  `gpu-context=androidvk`, `hwdec=mediacodec` falls back to software decoding, and
  `mediacodec-copy` copies every frame back to system memory. Either is a poor trade for 4K HEVC
  10-bit on the Xiaomi box (32-bit, low RAM), which is exactly the kind of file mpv is used for.
- **The one real gain is HDR output.** libplacebo's Vulkan swapchain implements the colour-space
  hint (`src/vulkan/swapchain.c`, `vk_sw_colorspace_hint`), so with `target-colorspace-hint=yes` it
  can ask for an HDR10 (PQ) surface. Its OpenGL swapchain has no such hint, and mpv's Android EGL
  context doesn't set one either, so on OpenGL ES `gpu-next` always tone-maps HDR to SDR. Whether a
  given device offers an HDR10 colour space on its Vulkan swapchain depends on the device and its
  display; it is untested on ours.
- **Ephyra doesn't need that gain from mpv.** HDR10 and Dolby Vision on HDR displays go through
  ExoPlayer, which hands the stream to the platform decoder and display. Nothing in Ephyra's mpv
  research (`docs/mpv-research/`) asks mpv for HDR output; its open questions are about the `Player`
  contract, not rendering. And HDR output that costs hardware decoding is no gain on the devices that
  matter.
- **Nobody else ships it.** mpv-android, the app these buildscripts come from, also builds with
  `-Dvulkan=disabled` and `--disable-vulkan`. Upstream started a `vulkan` branch in July 2025 and
  never merged it.

## Decision

Keep Vulkan disabled in libplacebo, mpv and FFmpeg. Don't add shaderc to the dependency tree.
Close #26 as not needed.

The AAR size and build time difference, and device tests with `hwdec=mediacodec` and HDR output,
were not measured: the gating question already fails on the hardware decoding point, so the numbers
couldn't change the outcome.

## Consequences

- The build, the AAR and the lines that differ from upstream stay as they are.
- `gpu-next` keeps tone-mapping HDR to SDR on mpv. Ephyra's HDR path remains ExoPlayer.
- `shaderc.sh` stays in the tree unused, as upstream has it.

## Revisit when

- mpv gains a Vulkan interop for MediaCodec frames (importing `AHardwareBuffer` through
  `VK_ANDROID_external_memory_android_hardware_buffer`), so Vulkan keeps zero-copy decoding, **and**
- Ephyra wants HDR output from mpv itself, for example because mpv becomes the default for files
  ExoPlayer can't play and some of those are HDR10.

Either one alone isn't enough. If both hold, start from upstream's `vulkan` branch, which already
builds shaderc and writes a `vulkan.pc` for the NDK. It needs two updates first: the `vulkan.pc`
version (mpv 0.42.0 requires `vulkan >= 1.4.309`; NDK 30's headers are 1.4.335), and the libplacebo
and mpv flags, which have moved on since. FFmpeg's Vulkan support stays off even then: mpv's
Vulkan output doesn't need it, it only serves `hwdec=vulkan` (Vulkan Video decoding). Ship it as
an app-side option in Ephyra with OpenGL ES as the default and a fallback when the Vulkan context
fails, then measure the AAR size, the CI time, and HDR10 and Dolby Vision playback on the phone and
the box.

## Alternatives considered

- **Enable Vulkan, OpenGL ES stays the default.** The app picks `gpu-context=androidvk` behind an
  option. Rejected for now: it costs a shaderc build in CI for all four ABIs, larger `.so` files for
  every user, and more lines that differ from upstream, for an option that loses hardware decoding
  when switched on.
- **`vo=mediacodec_embed` for HDR on mpv.** The decoder renders straight to the surface, so the
  platform handles HDR output, and it works with today's build. Not part of this decision: mpv then
  draws no subtitles or OSD over the video, which defeats a main reason to use mpv. If Ephyra wants
  HDR output from mpv before the conditions above hold, evaluate this in Ephyra first; it needs no
  change here.
