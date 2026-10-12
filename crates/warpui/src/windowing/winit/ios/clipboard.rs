// moca: ios
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

use crate::Clipboard;
use crate::clipboard::ClipboardContent;

/// Portapapeles de iOS basado en UIPasteboard; solo maneja texto.
pub struct IosClipboard;

impl Clipboard for IosClipboard {
    fn write(&mut self, contents: ClipboardContent) {
        if contents.plain_text.is_empty() {
            return;
        }
        let pasteboard: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(UIPasteboard), generalPasteboard] };
        let Some(pasteboard) = pasteboard else { return };
        let ns = NSString::from_str(&contents.plain_text);
        let _: () = unsafe { msg_send![&*pasteboard, setString: &*ns] };
    }

    fn read(&mut self) -> ClipboardContent {
        let pasteboard: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(UIPasteboard), generalPasteboard] };
        let plain_text = pasteboard
            .and_then(|pasteboard| {
                let text: Option<Retained<NSString>> = unsafe { msg_send![&*pasteboard, string] };
                text.map(|t| t.to_string())
            })
            .unwrap_or_default();
        ClipboardContent {
            plain_text,
            ..Default::default()
        }
    }
}
