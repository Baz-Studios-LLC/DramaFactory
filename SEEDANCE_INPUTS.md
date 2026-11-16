# Seedance inputs and local portrait transport

Verified against official OpenRouter documentation on 2026-11-16. No files were uploaded and no paid requests were made for this investigation.

## Supported request paths

The [reference-to-video guide](https://openrouter.ai/docs/cookbook/video-generation/reference-to-video) uses directly downloadable public HTTPS images in `input_references`. The [video request reference](https://openrouter.ai/docs/api/api-reference/video-generation/submit-a-video-generation-request) exposes `frame_images` for first/last frames and separate image/audio/video reference shapes. Provider support must be checked; not every reference type is honored everywhere.

The current app supports HTTPS image URLs and mutually exclusive reference/frame modes. Saved local character references bind exact selected portrait hashes, but cannot yet be transported to the provider. They intentionally block submission instead of being silently omitted.

## Ruled-out upload approach

OpenRouter's [Files API](https://openrouter.ai/docs/guides/features/files-api) accepts image uploads, but its download rules explicitly restrict uploaded files to OpenRouter sandboxes. Uploaded files cannot be fetched using the file content endpoint. The documented video schema does not provide a workspace file-ID input. Therefore uploading to `/api/v1/files` does not establish a usable video-reference path.

Do not implement a Files API upload as if it solved video references. Do not assume that data URLs supported by image generation are also accepted by the video route. No documented video data-URL acceptance was found in this review; that is an evidence gap, not proof of rejection.

## Outstanding work

A provider-supported local-image transport must be established before automatic portrait-guided episode generation can be claimed. External hosting would change the all-OpenRouter design and expose reference media; it has not been configured or deployed. A future supported transport needs bounded original-byte upload, exact hash binding, durable receipts, expiry handling, cancellation/recovery and loopback tests, followed by explicit user-authorized live quality testing. No paid test or external hosting is authorized by this document.
