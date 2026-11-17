Based on egui-winit 0.33.3 (MIT OR Apache-2.0).

The keyboard paste handler emits Event::Paste even when clipboard text is empty. This preserves image-only paste intent on key-down and removes dependence on macOS Command-key release order. No clipboard contents are transmitted. Keep this patch until upstream exposes non-text paste intent.
