#![allow(clippy::unnecessary_cast)]
use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{CGFloat, CGPoint, CGRect, MainThreadMarker, NSObject, NSSet, NSString};
use objc2_ui_kit::{
    UICoordinateSpace, UIEvent, UIForceTouchCapability, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer, UIKeyInput,
    UIPanGestureRecognizer, UIScrollTypeMask,
    UIPinchGestureRecognizer, UIPress, UIPressesEvent, UIResponder, UIRotationGestureRecognizer, UITapGestureRecognizer,
    UITextInputTraits, UITouch, UITouchPhase, UITouchType, UITraitEnvironment, UIView,
};

use super::app_state::{self, EventWrapper};
use super::keyboard;
use super::window::WinitUIWindow;
use crate::dpi::PhysicalPosition;
use crate::event::{
    ElementState, Event, Force, KeyEvent, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use crate::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::{WindowAttributes, WindowId as RootWindowId};

pub struct WinitViewState {
    pinch_gesture_recognizer: RefCell<Option<Retained<UIPinchGestureRecognizer>>>,
    doubletap_gesture_recognizer: RefCell<Option<Retained<UITapGestureRecognizer>>>,
    rotation_gesture_recognizer: RefCell<Option<Retained<UIRotationGestureRecognizer>>>,
    pan_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,

    // for iOS delta references the start of the Gesture
    rotation_last_delta: Cell<CGFloat>,
    pinch_last_delta: Cell<CGFloat>,
    pan_last_delta: Cell<CGPoint>,

    // moca: ios — puntero del trackpad/ratón: posición sin clic y scroll con dos dedos.
    hover_gesture_recognizer: RefCell<Option<Retained<UIHoverGestureRecognizer>>>,
    scroll_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,
    scroll_last_translation: Cell<CGPoint>,

    // moca: ios — teclado físico: modificadores vigentes y tecla que se repite al mantenerla.
    modifiers: Cell<ModifiersState>,
    repeating_key: RefCell<Option<(isize, KeyEvent)>>,
}

/// moca: ios — retardo antes de repetir una tecla mantenida y ritmo de repetición (segundos).
const KEY_REPEAT_DELAY: f64 = 0.4;
const KEY_REPEAT_INTERVAL: f64 = 0.05;

declare_class!(
    pub(crate) struct WinitView;

    unsafe impl ClassType for WinitView {
        #[inherits(UIResponder, NSObject)]
        type Super = UIView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitUIView";
    }

    impl DeclaredClass for WinitView {
        type Ivars = WinitViewState;
    }

    unsafe impl WinitView {
        #[method(drawRect:)]
        fn draw_rect(&self, rect: CGRect) {
            let mtm = MainThreadMarker::new().unwrap();
            let window = self.window().unwrap();
            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::RedrawRequested,
                }),
            );
            let _: () = unsafe { msg_send![super(self), drawRect: rect] };
        }

        #[method(layoutSubviews)]
        fn layout_subviews(&self) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };

            let window = self.window().unwrap();
            let window_bounds = window.bounds();
            let screen = window.screen();
            let screen_space = screen.coordinateSpace();
            let screen_frame = self.convertRect_toCoordinateSpace(window_bounds, &screen_space);
            let scale_factor = screen.scale();
            let size = crate::dpi::LogicalSize {
                width: screen_frame.size.width as f64,
                height: screen_frame.size.height as f64,
            }
            .to_physical(scale_factor as f64);

            // If the app is started in landscape, the view frame and window bounds can be mismatched.
            // The view frame will be in portrait and the window bounds in landscape. So apply the
            // window bounds to the view frame to make it consistent.
            let view_frame = self.frame();
            if view_frame != window_bounds {
                self.setFrame(window_bounds);
            }

            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::Resized(size),
                }),
            );
        }

        #[method(setContentScaleFactor:)]
        fn set_content_scale_factor(&self, untrusted_scale_factor: CGFloat) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () =
                unsafe { msg_send![super(self), setContentScaleFactor: untrusted_scale_factor] };

            // `window` is null when `setContentScaleFactor` is invoked prior to `[UIWindow
            // makeKeyAndVisible]` at window creation time (either manually or internally by
            // UIKit when the `UIView` is first created), in which case we send no events here
            let window = match self.window() {
                Some(window) => window,
                None => return,
            };
            // `setContentScaleFactor` may be called with a value of 0, which means "reset the
            // content scale factor to a device-specific default value", so we can't use the
            // parameter here. We can query the actual factor using the getter
            let scale_factor = self.contentScaleFactor();
            assert!(
                !scale_factor.is_nan()
                    && scale_factor.is_finite()
                    && scale_factor.is_sign_positive()
                    && scale_factor > 0.0,
                "invalid scale_factor set on UIView",
            );
            let scale_factor = scale_factor as f64;
            let bounds = self.bounds();
            let screen = window.screen();
            let screen_space = screen.coordinateSpace();
            let screen_frame = self.convertRect_toCoordinateSpace(bounds, &screen_space);
            let size = crate::dpi::LogicalSize {
                width: screen_frame.size.width as f64,
                height: screen_frame.size.height as f64,
            };
            let window_id = RootWindowId(window.id());
            app_state::handle_nonuser_events(
                mtm,
                std::iter::once(EventWrapper::ScaleFactorChanged(
                    app_state::ScaleFactorChanged {
                        window,
                        scale_factor,
                        suggested_size: size.to_physical(scale_factor),
                    },
                ))
                .chain(std::iter::once(EventWrapper::StaticEvent(
                    Event::WindowEvent {
                        window_id,
                        event: WindowEvent::Resized(size.to_physical(scale_factor)),
                    },
                ))),
            );
        }

        #[method(touchesBegan:withEvent:)]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            // moca: ios — Warp activa botones según el hover; sin un CursorMoved previo, el
            // primer clic sobre un botón (o un toque) no "estaba encima" y se ignoraba.
            if let Some(touch) = unsafe { touches.anyObject() } {
                self.send_cursor_moved(touch.locationInView(Some(self)));
            }
            self.handle_touches(touches)
        }

        #[method(touchesMoved:withEvent:)]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.handle_touches(touches)
        }

        #[method(touchesEnded:withEvent:)]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.handle_touches(touches)
        }

        #[method(touchesCancelled:withEvent:)]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.handle_touches(touches)
        }

        #[method(pinchGesture:)]
        fn pinch_gesture(&self, recognizer: &UIPinchGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pinch_last_delta.set(recognizer.scale());
                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(recognizer.scale());
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Ended => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(0.0);
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);
                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.scale())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: delta as f64,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(doubleTapGesture:)]
        fn double_tap_gesture(&self, recognizer: &UITapGestureRecognizer) {
            let window = self.window().unwrap();

            if recognizer.state() == UIGestureRecognizerState::Ended {
                let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::DoubleTapGesture {
                        device_id: DEVICE_ID,
                    },
                });

                let mtm = MainThreadMarker::new().unwrap();
                app_state::handle_nonuser_event(mtm, gesture_event);
            }
        }

        #[method(rotationGesture:)]
        fn rotation_gesture(&self, recognizer: &UIRotationGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().rotation_last_delta.set(0.0);

                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(recognizer.rotation());

                    (TouchPhase::Moved, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Ended => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(0.0);

                    (TouchPhase::Ended, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.rotation())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            // Make delta negative to match macos, convert to degrees
            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::RotationGesture {
                    device_id: DEVICE_ID,
                    delta: -delta.to_degrees() as _,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(panGesture:)]
        fn pan_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let window = self.window().unwrap();

            let translation = recognizer.translationInView(Some(self));

            let (phase, dx, dy) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pan_last_delta.set(translation);

                    (TouchPhase::Started, 0.0, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(translation);

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Moved, dx, dy)
                }
                UIGestureRecognizerState::Ended => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Ended, dx, dy)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -last_pan.x, -last_pan.y)
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };


            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PanGesture {
                    device_id: DEVICE_ID,
                    delta: PhysicalPosition::new(dx as _, dy as _),
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        // moca: ios — el puntero se mueve sin clic: se informa como CursorMoved (hover).
        #[method(hoverGesture:)]
        fn hover_gesture(&self, recognizer: &UIHoverGestureRecognizer) {
            match recognizer.state() {
                UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed => {
                    let location = unsafe { recognizer.locationInView(Some(self)) };
                    self.send_cursor_moved(location);
                }
                _ => {}
            }
        }

        // moca: ios — scroll con dos dedos en el trackpad (o rueda del ratón): MouseWheel en
        // píxeles. El desplazamiento sigue a los dedos, igual que el arrastre táctil.
        #[method(scrollGesture:)]
        fn scroll_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let translation = recognizer.translationInView(Some(self));
            let (phase, dx, dy) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().scroll_last_translation.set(translation);
                    (TouchPhase::Started, 0.0, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last = self.ivars().scroll_last_translation.replace(translation);
                    (TouchPhase::Moved, translation.x - last.x, translation.y - last.y)
                }
                UIGestureRecognizerState::Ended => {
                    let last = self
                        .ivars()
                        .scroll_last_translation
                        .replace(CGPoint { x: 0.0, y: 0.0 });
                    (TouchPhase::Ended, translation.x - last.x, translation.y - last.y)
                }
                _ => {
                    self.ivars().scroll_last_translation.set(CGPoint { x: 0.0, y: 0.0 });
                    (TouchPhase::Cancelled, 0.0, 0.0)
                }
            };
            let window = self.window().unwrap();
            let scale_factor = self.contentScaleFactor();
            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::MouseWheel {
                        device_id: DEVICE_ID,
                        delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                            (dx * scale_factor) as f64,
                            (dy * scale_factor) as f64,
                        )),
                        phase,
                    },
                }),
            );
        }

        // moca: ios — teclado físico. UIKit solo manda texto (insertText) y Backspace
        // (deleteBackward); flechas, Escape y atajos con Ctrl/⌘ llegan solo aquí.
        #[method(pressesBegan:withEvent:)]
        fn presses_began(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Pressed) {
                unsafe { msg_send![super(self), pressesBegan: presses, withEvent: event] }
            }
        }

        #[method(pressesEnded:withEvent:)]
        fn presses_ended(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Released) {
                unsafe { msg_send![super(self), pressesEnded: presses, withEvent: event] }
            }
        }

        #[method(pressesCancelled:withEvent:)]
        fn presses_cancelled(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Released) {
                unsafe { msg_send![super(self), pressesCancelled: presses, withEvent: event] }
            }
        }

        #[method(mocaKeyRepeat)]
        fn moca_key_repeat(&self) {
            let Some((_, event)) = self.ivars().repeating_key.borrow().clone() else { return };
            self.send_key_event(event);
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(mocaKeyRepeat),
                    withObject: std::ptr::null::<AnyObject>(),
                    afterDelay: KEY_REPEAT_INTERVAL
                ];
            }
        }

        #[method(canBecomeFirstResponder)]
        fn can_become_first_responder(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for WinitView {}

    unsafe impl UIGestureRecognizerDelegate for WinitView {
        #[method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:)]
        fn should_recognize_simultaneously(&self, _gesture_recognizer: &UIGestureRecognizer, _other_gesture_recognizer: &UIGestureRecognizer) -> bool {
            true
        }
    }

    unsafe impl UITextInputTraits for WinitView {
    }

    unsafe impl UIKeyInput for WinitView {
        #[method(hasText)]
        fn has_text(&self) -> bool {
            true
        }

        #[method(insertText:)]
        fn insert_text(&self, text: &NSString) {
            self.handle_insert_text(text)
        }

        #[method(deleteBackward)]
        fn delete_backward(&self) {
            self.handle_delete_backward()
        }
    }
);

impl WinitView {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        window_attributes: &WindowAttributes,
        frame: CGRect,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(WinitViewState {
            pinch_gesture_recognizer: RefCell::new(None),
            doubletap_gesture_recognizer: RefCell::new(None),
            rotation_gesture_recognizer: RefCell::new(None),
            pan_gesture_recognizer: RefCell::new(None),

            rotation_last_delta: Cell::new(0.0),
            pinch_last_delta: Cell::new(0.0),
            pan_last_delta: Cell::new(CGPoint { x: 0.0, y: 0.0 }),

            hover_gesture_recognizer: RefCell::new(None),
            scroll_gesture_recognizer: RefCell::new(None),
            scroll_last_translation: Cell::new(CGPoint { x: 0.0, y: 0.0 }),

            modifiers: Cell::new(ModifiersState::empty()),
            repeating_key: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), initWithFrame: frame] };

        this.setMultipleTouchEnabled(true);
        this.install_pointer_recognizers();

        if let Some(scale_factor) = window_attributes.platform_specific.scale_factor {
            this.setContentScaleFactor(scale_factor as _);
        }

        this
    }

    fn window(&self) -> Option<Retained<WinitUIWindow>> {
        // SAFETY: `WinitView`s are always installed in a `WinitUIWindow`
        (**self).window().map(|window| unsafe { Retained::cast(window) })
    }

    /// moca: ios — convierte pulsaciones del teclado físico en eventos de winit. Devuelve `true`
    /// si las consumió; si no, deben seguir a UIKit, que las entrega como texto (`insertText`,
    /// con acentos y teclas muertas) o como `deleteBackward`, ambos con repetición propia.
    fn handle_presses(&self, presses: &NSSet<UIPress>, state: ElementState) -> bool {
        let mtm = MainThreadMarker::from(self);
        let mut consumed = false;
        for press in presses {
            let Some(key) = (unsafe { press.key(mtm) }) else { continue };
            let usage = unsafe { key.keyCode() }.0 as isize;
            let flags = unsafe { key.modifierFlags() }.0 as isize;

            let mut modifiers = keyboard::modifiers_from_flags(flags);
            if keyboard::is_modifier_hid(usage) {
                // Al soltar un modificador, sus flags pueden seguir marcándolo: se quita a mano.
                let own = keyboard::modifiers_from_flags(match usage {
                    0xE0 | 0xE4 => keyboard::MOD_CONTROL,
                    0xE1 | 0xE5 => keyboard::MOD_SHIFT,
                    0xE2 | 0xE6 => keyboard::MOD_ALT,
                    _ => keyboard::MOD_COMMAND,
                });
                match state {
                    ElementState::Pressed => modifiers.insert(own),
                    ElementState::Released => modifiers.remove(own),
                }
            }
            self.update_modifiers(modifiers);
            if keyboard::is_modifier_hid(usage) {
                continue;
            }

            let chord = modifiers.intersects(ModifiersState::CONTROL | ModifiersState::SUPER);
            let named = keyboard::named_key_from_hid(usage)
                .filter(|named| *named != NamedKey::Backspace || chord);
            if named.is_none() && !chord {
                // Texto normal (con o sin Shift/Option): lo maneja UIKit.
                continue;
            }

            let logical_key = match named {
                Some(named) => Key::Named(named),
                None => {
                    let chars = unsafe { key.charactersIgnoringModifiers() }.to_string();
                    if chars.is_empty() {
                        continue;
                    }
                    Key::Character(smol_str::SmolStr::new(chars.to_lowercase()))
                }
            };
            let text = match (state, named) {
                (ElementState::Pressed, Some(NamedKey::Enter)) => Some("\r"),
                (ElementState::Pressed, Some(NamedKey::Tab)) => Some("\t"),
                (ElementState::Pressed, Some(NamedKey::Escape)) => Some("\u{1b}"),
                _ => None,
            }
            .map(smol_str::SmolStr::new);
            let event = KeyEvent {
                physical_key: keyboard::key_code_from_hid(usage)
                    .map(PhysicalKey::Code)
                    .unwrap_or(PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
                logical_key,
                text,
                location: KeyLocation::Standard,
                state,
                repeat: false,
                platform_specific: KeyEventExtra {},
            };

            unsafe {
                let _: () = msg_send![
                    NSObject::class(),
                    cancelPreviousPerformRequestsWithTarget: self
                ];
            }
            match state {
                ElementState::Pressed => {
                    let mut repeat = event.clone();
                    repeat.repeat = true;
                    self.ivars().repeating_key.replace(Some((usage, repeat)));
                    unsafe {
                        let _: () = msg_send![
                            self,
                            performSelector: sel!(mocaKeyRepeat),
                            withObject: std::ptr::null::<AnyObject>(),
                            afterDelay: KEY_REPEAT_DELAY
                        ];
                    }
                }
                ElementState::Released => {
                    self.ivars().repeating_key.replace(None);
                }
            }
            self.send_key_event(event);
            consumed = true;
        }
        consumed
    }

    fn update_modifiers(&self, modifiers: ModifiersState) {
        if self.ivars().modifiers.replace(modifiers) == modifiers {
            return;
        }
        let Some(window) = self.window() else { return };
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_event(
            mtm,
            EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::ModifiersChanged(modifiers.into()),
            }),
        );
    }

    fn send_key_event(&self, event: KeyEvent) {
        let Some(window) = self.window() else { return };
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_event(
            mtm,
            EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event,
                    is_synthetic: false,
                },
            }),
        );
    }

    /// moca: ios — reconocedores del puntero indirecto (trackpad/ratón del Magic Keyboard).
    fn install_pointer_recognizers(&self) {
        let mtm = MainThreadMarker::from(self);
        let hover = unsafe {
            UIHoverGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(hoverGesture:)),
            )
        };
        hover.setDelegate(Some(ProtocolObject::from_ref(self)));
        self.addGestureRecognizer(&hover);
        self.ivars().hover_gesture_recognizer.replace(Some(hover));

        let scroll = unsafe {
            UIPanGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(scrollGesture:)),
            )
        };
        scroll.setDelegate(Some(ProtocolObject::from_ref(self)));
        // Cero toques: solo scroll del trackpad/rueda, nunca arrastres con el dedo (esos ya
        // llegan como Touch y Warp los convierte en scroll o selección).
        scroll.setMaximumNumberOfTouches(0);
        unsafe { scroll.setAllowedScrollTypesMask(UIScrollTypeMask::All) };
        self.addGestureRecognizer(&scroll);
        self.ivars().scroll_gesture_recognizer.replace(Some(scroll));
    }

    /// moca: ios — posición del puntero (en puntos de la vista) como `CursorMoved`.
    fn send_cursor_moved(&self, location: CGPoint) {
        let Some(window) = self.window() else { return };
        let scale_factor = self.contentScaleFactor();
        let position = PhysicalPosition::from_logical::<(f64, f64), f64>(
            (location.x as _, location.y as _),
            scale_factor as f64,
        );
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_event(
            mtm,
            EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::CursorMoved { device_id: DEVICE_ID, position },
            }),
        );
    }

    pub(crate) fn recognize_pinch_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pinch_gesture_recognizer.borrow().is_none() {
                let pinch = unsafe {
                    UIPinchGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(pinchGesture:)),
                    )
                };
                pinch.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&pinch);
                self.ivars().pinch_gesture_recognizer.replace(Some(pinch));
            }
        } else if let Some(recognizer) = self.ivars().pinch_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_pan_gesture(
        &self,
        should_recognize: bool,
        minimum_number_of_touches: u8,
        maximum_number_of_touches: u8,
    ) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pan_gesture_recognizer.borrow().is_none() {
                let pan = unsafe {
                    UIPanGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(panGesture:)),
                    )
                };
                pan.setDelegate(Some(ProtocolObject::from_ref(self)));
                pan.setMinimumNumberOfTouches(minimum_number_of_touches as _);
                pan.setMaximumNumberOfTouches(maximum_number_of_touches as _);
                self.addGestureRecognizer(&pan);
                self.ivars().pan_gesture_recognizer.replace(Some(pan));
            }
        } else if let Some(recognizer) = self.ivars().pan_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_doubletap_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().doubletap_gesture_recognizer.borrow().is_none() {
                let tap = unsafe {
                    UITapGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(doubleTapGesture:)),
                    )
                };
                tap.setDelegate(Some(ProtocolObject::from_ref(self)));
                tap.setNumberOfTapsRequired(2);
                tap.setNumberOfTouchesRequired(1);
                self.addGestureRecognizer(&tap);
                self.ivars().doubletap_gesture_recognizer.replace(Some(tap));
            }
        } else if let Some(recognizer) = self.ivars().doubletap_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_rotation_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().rotation_gesture_recognizer.borrow().is_none() {
                let rotation = unsafe {
                    UIRotationGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(rotationGesture:)),
                    )
                };
                rotation.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&rotation);
                self.ivars().rotation_gesture_recognizer.replace(Some(rotation));
            }
        } else if let Some(recognizer) = self.ivars().rotation_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    fn handle_touches(&self, touches: &NSSet<UITouch>) {
        let window = self.window().unwrap();
        let mut touch_events = Vec::new();
        let os_supports_force = app_state::os_capabilities().force_touch;
        for touch in touches {
            let logical_location = touch.locationInView(None);
            let touch_type = touch.r#type();
            let force = if os_supports_force {
                let trait_collection = self.traitCollection();
                let touch_capability = trait_collection.forceTouchCapability();
                // Both the OS _and_ the device need to be checked for force touch support.
                if touch_capability == UIForceTouchCapability::Available
                    || touch_type == UITouchType::Pencil
                {
                    let force = touch.force();
                    let max_possible_force = touch.maximumPossibleForce();
                    let altitude_angle: Option<f64> = if touch_type == UITouchType::Pencil {
                        let angle = touch.altitudeAngle();
                        Some(angle as _)
                    } else {
                        None
                    };
                    Some(Force::Calibrated {
                        force: force as _,
                        max_possible_force: max_possible_force as _,
                        altitude_angle,
                    })
                } else {
                    None
                }
            } else {
                None
            };
            let touch_id = touch as *const UITouch as u64;
            let phase = touch.phase();
            let phase = match phase {
                UITouchPhase::Began => TouchPhase::Started,
                UITouchPhase::Moved => TouchPhase::Moved,
                // 2 is UITouchPhase::Stationary and is not expected here
                UITouchPhase::Ended => TouchPhase::Ended,
                UITouchPhase::Cancelled => TouchPhase::Cancelled,
                _ => panic!("unexpected touch phase: {phase:?}"),
            };

            let physical_location = {
                let scale_factor = self.contentScaleFactor();
                PhysicalPosition::from_logical::<(f64, f64), f64>(
                    (logical_location.x as _, logical_location.y as _),
                    scale_factor as f64,
                )
            };
            touch_events.push(EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::Touch(Touch {
                    device_id: DEVICE_ID,
                    id: touch_id,
                    location: physical_location,
                    force,
                    phase,
                }),
            }));
        }
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_events(mtm, touch_events);
    }

    fn handle_insert_text(&self, text: &NSString) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::new().unwrap();
        // send individual events for each character
        app_state::handle_nonuser_events(
            mtm,
            text.to_string().chars().flat_map(|c| {
                let text = smol_str::SmolStr::from_iter([c]);
                // Emit both press and release events
                [ElementState::Pressed, ElementState::Released].map(|state| {
                    EventWrapper::StaticEvent(Event::WindowEvent {
                        window_id,
                        event: WindowEvent::KeyboardInput {
                            event: KeyEvent {
                                text: if state == ElementState::Pressed {
                                    Some(text.clone())
                                } else {
                                    None
                                },
                                state,
                                location: KeyLocation::Standard,
                                repeat: false,
                                logical_key: Key::Character(text.clone()),
                                physical_key: PhysicalKey::Unidentified(
                                    NativeKeyCode::Unidentified,
                                ),
                                platform_specific: KeyEventExtra {},
                            },
                            is_synthetic: false,
                            device_id: DEVICE_ID,
                        },
                    })
                })
            }),
        );
    }

    fn handle_delete_backward(&self) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_events(
            mtm,
            [ElementState::Pressed, ElementState::Released].map(|state| {
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id,
                    event: WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event: KeyEvent {
                            state,
                            logical_key: Key::Named(NamedKey::Backspace),
                            physical_key: PhysicalKey::Code(KeyCode::Backspace),
                            platform_specific: KeyEventExtra {},
                            repeat: false,
                            location: KeyLocation::Standard,
                            text: None,
                        },
                        is_synthetic: false,
                    },
                })
            }),
        );
    }
}
