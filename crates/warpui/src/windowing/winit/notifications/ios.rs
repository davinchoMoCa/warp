use winit::event_loop::EventLoopProxy;

use crate::WindowId;
use crate::platform::NotificationInfo;
use crate::windowing::winit::app::CustomEvent;

// moca: ios - system notifications will come later from the Swift app layer.
pub(super) async fn send_notification(
    _notification_info: NotificationInfo,
    _window_id: WindowId,
    _proxy: EventLoopProxy<CustomEvent>,
) {
}
