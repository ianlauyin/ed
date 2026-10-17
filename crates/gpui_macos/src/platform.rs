use crate::{
    AppleActivity, AppleDispatcher, MacDisplay, MacKeyboardLayout, MacKeyboardMapper, MacWindow,
    events::key_to_native,
    ns::{
        NSInteger, NSOperatingSystemVersion, NSUInteger, array_iter, autorelease_pool, drain, id,
        nil,
    },
    ns_string,
    pasteboard::Pasteboard,
    renderer, set_active_window_cursor_style,
    window_appearance::{
        NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSAppearanceNameVibrantDark,
        NSAppearanceNameVibrantLight,
    },
};
use anyhow::{Context as _, anyhow};
use block2::RcBlock;
use core_foundation::{base::CFRelease, runloop::CFRunLoopRun, string::CFStringRef};
use dispatch2::DispatchQueue;
use futures::channel::oneshot;
use gpui::{
    Action, ActivationPolicy, ActivityGuard, AnyWindowHandle, BackgroundExecutor, ClipboardItem,
    CursorStyle, ForegroundExecutor, GraphicalEnvironment, KeyContext, Keymap, Menu, MenuItem,
    OsMenu, OwnedMenu, PathPromptOptions, Platform, PlatformDisplay, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Result, SystemMenuType, Task,
    ThermalState, WindowAppearance, WindowKind, WindowParams, WindowingRequest,
    popup::PopupNotSupportedError,
};
use gpui_apple::{
    keychain,
    thermal::{self, ThermalObserver},
};
use gpui_util::{ResultExt, new_std_command};
use itertools::Itertools;
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, class, define_class, msg_send,
    runtime::{AnyClass, AnyObject, NSObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSEventModifierFlags, NSModalResponse, NSModalResponseOK, NSOpenPanel,
    NSResponder, NSSavePanel, NSWorkspace, NSWorkspaceDidWakeNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::NSActivityOptions;
use parking_lot::Mutex;
use semver::Version;
use std::{
    cell::Cell,
    ffi::{CStr, c_void},
    os::{raw::c_char, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
    ptr,
    rc::Rc,
    slice, str,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[allow(non_upper_case_globals)]
const NSUTF8StringEncoding: NSUInteger = 4;

/// Instance variables of `GPUIApplication` and `GPUIApplicationDelegate`.
///
/// AppKit allocates the application itself (through `sharedApplication`), so these are never
/// set up through `set_ivars`: they must stay valid when zero-filled and must not need `Drop`.
struct PlatformIvars {
    /// The running `MacPlatform`, set for the duration of `MacPlatform::run`.
    platform: Cell<*const MacPlatform>,
}

define_class!(
    // SAFETY: `NSApplication` has no subclassing requirements, and `PlatformIvars` is valid
    // zero-filled and has no `Drop`.
    #[unsafe(super(NSApplication, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIApplication"]
    #[ivars = PlatformIvars]
    struct GPUIApplication;
);

define_class!(
    // SAFETY: `NSResponder` has no subclassing requirements, and `PlatformIvars` is valid
    // zero-filled and has no `Drop`.
    #[unsafe(super(NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIApplicationDelegate"]
    #[ivars = PlatformIvars]
    struct GPUIApplicationDelegate;

    impl GPUIApplicationDelegate {
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn application_will_finish_launching(&self, notification: id) {
            will_finish_launching(self, notification);
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, notification: id) {
            did_finish_launching(self, notification);
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(&self, app: id, has_visible_windows: bool) {
            should_handle_reopen(self, app, has_visible_windows);
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, notification: id) {
            will_terminate(self, notification);
        }

        #[unsafe(method(handleGPUIMenuItem:))]
        fn handle_gpui_menu_item(&self, item: id) {
            handle_menu_item(self, item);
        }

        // Menu item handlers so that OS save panels have the correct key commands.
        #[unsafe(method(cut:))]
        fn cut(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(copy:))]
        fn copy(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(paste:))]
        fn paste(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(undo:))]
        fn undo(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(redo:))]
        fn redo(&self, item: id) {
            handle_menu_item(self, item);
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item_(&self, item: id) -> bool {
            validate_menu_item(self, item)
        }

        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open_(&self, menu: id) {
            menu_will_open(self, menu);
        }

        #[unsafe(method(applicationDockMenu:))]
        fn application_dock_menu(&self, app: id) -> id {
            handle_dock_menu(self, app)
        }

        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, app: id, urls: id) {
            open_urls(self, app, urls);
        }

        #[unsafe(method(onKeyboardLayoutChange:))]
        fn on_keyboard_layout_change_(&self, notification: id) {
            on_keyboard_layout_change(self, notification);
        }

        #[unsafe(method(onScreenParametersChange:))]
        fn on_screen_parameters_change_(&self, notification: id) {
            on_screen_parameters_change(self, notification);
        }

        #[unsafe(method(onSystemSleep:))]
        fn on_system_sleep_(&self, notification: id) {
            on_system_sleep(self, notification);
        }

        #[unsafe(method(onSystemWake:))]
        fn on_system_wake_(&self, notification: id) {
            on_system_wake(self, notification);
        }
    }
);

pub struct MacPlatform(Mutex<MacPlatformState>, MainThreadMarker);

pub(crate) struct MacPlatformState {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    renderer_context: renderer::Context,
    headless: bool,
    activation_policy: ActivationPolicy,
    application_created: bool,
    general_pasteboard: Pasteboard,
    find_pasteboard: Pasteboard,
    reopen: Option<Box<dyn FnMut()>>,
    on_keyboard_layout_change: Option<Box<dyn FnMut()>>,
    thermal_observer: Option<ThermalObserver>,
    on_displays_changed: Option<Box<dyn FnMut()>>,
    on_system_sleep: Option<Box<dyn FnMut()>>,
    on_system_wake: Option<Box<dyn FnMut()>>,
    system_power_observers_registered: bool,
    quit: Option<Box<dyn FnMut() -> bool>>,
    menu_command: Option<Box<dyn FnMut(&dyn Action)>>,
    validate_menu_command: Option<Box<dyn FnMut(&dyn Action) -> bool>>,
    will_open_menu: Option<Box<dyn FnMut()>>,
    menu_actions: Vec<Box<dyn Action>>,
    open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
    finish_launching: Option<Box<dyn FnOnce()>>,
    dock_menu: Option<id>,
    menus: Option<Vec<OwnedMenu>>,
    keyboard_mapper: Rc<MacKeyboardMapper>,
    /// Mirrors `[NSCursor setHiddenUntilMouseMoves:]` state, which AppKit doesn't expose.
    cursor_visible: Arc<AtomicBool>,
}

/// The activation policy that stands for a windowing mode: an app is headless while it has no
/// Dock icon or menu bar.
fn activation_policy_for(request: &WindowingRequest) -> ActivationPolicy {
    match request {
        WindowingRequest::Headless => ActivationPolicy::Accessory,
        WindowingRequest::Windowed(_) => ActivationPolicy::Regular,
    }
}

/// `NSApplicationActivationPolicy` raw value.
fn native_activation_policy(policy: ActivationPolicy) -> NSInteger {
    match policy {
        ActivationPolicy::Regular => 0,
        ActivationPolicy::Accessory => 1,
    }
}

unsafe fn shared_application() -> id {
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

/// Returns an autoreleased `[[class alloc] init]`.
unsafe fn new_autoreleased(class: &AnyClass) -> id {
    unsafe {
        let object: id = msg_send![class, new];
        msg_send![object, autorelease]
    }
}

/// Returns an autoreleased `NSMenuItem`.
unsafe fn new_menu_item(title: &str, action: Sel, key_equivalent: &str) -> id {
    unsafe {
        let item: id = msg_send![class!(NSMenuItem), alloc];
        let item: id = msg_send![
            item,
            initWithTitle: ns_string(title),
            action: action,
            keyEquivalent: ns_string(key_equivalent)
        ];
        msg_send![item, autorelease]
    }
}

impl MacPlatform {
    pub fn new(headless: bool) -> Self {
        let marker = MainThreadMarker::new().expect("Mac platform not created on main thread");
        let dispatcher = Arc::new(AppleDispatcher::new());

        #[cfg(feature = "font-kit")]
        let text_system = Arc::new(gpui_apple::AppleTextSystem::new());

        #[cfg(not(feature = "font-kit"))]
        let text_system = {
            if !headless {
                log::warn!(
                    "gpui_macos was compiled without the `font-kit` feature, so no text will be rendered."
                );
            }
            Arc::new(gpui::NoopTextSystem::new())
        };

        let keyboard_layout = MacKeyboardLayout::new();
        let keyboard_mapper = Rc::new(MacKeyboardMapper::new(keyboard_layout.id()));

        let state = Mutex::new(MacPlatformState {
            headless,
            activation_policy: ActivationPolicy::Regular,
            application_created: false,
            text_system,
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            renderer_context: renderer::Context::default(),
            general_pasteboard: Pasteboard::general(),
            find_pasteboard: Pasteboard::find(),
            reopen: None,
            quit: None,
            menu_command: None,
            validate_menu_command: None,
            will_open_menu: None,
            menu_actions: Default::default(),
            open_urls: None,
            finish_launching: None,
            dock_menu: None,
            on_keyboard_layout_change: None,
            thermal_observer: None,
            on_displays_changed: None,
            on_system_sleep: None,
            on_system_wake: None,
            system_power_observers_registered: false,
            menus: None,
            keyboard_mapper,
            cursor_visible: Arc::new(AtomicBool::new(true)),
        });
        Self(state, marker)
    }

    unsafe fn create_menu_bar(
        &self,
        menus: &Vec<Menu>,
        delegate: id,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> id {
        unsafe {
            let application_menu = new_autoreleased(class!(NSMenu));
            let _: () = msg_send![application_menu, setDelegate: delegate];

            for menu_config in menus {
                let menu = new_autoreleased(class!(NSMenu));
                let menu_title = ns_string(&menu_config.name);
                let _: () = msg_send![menu, setTitle: menu_title];
                let _: () = msg_send![menu, setDelegate: delegate];

                for item_config in &menu_config.items {
                    let menu_item = Self::create_menu_item(item_config, delegate, actions, keymap);
                    let _: () = msg_send![menu, addItem: menu_item];
                }

                let menu_item = new_autoreleased(class!(NSMenuItem));
                let _: () = msg_send![menu_item, setTitle: menu_title];
                let _: () = msg_send![menu_item, setSubmenu: menu];
                let _: () = msg_send![application_menu, addItem: menu_item];

                if menu_config.name == "Window" {
                    let app: id = msg_send![GPUIApplication::class(), sharedApplication];
                    let _: () = msg_send![app, setWindowsMenu: menu];
                }
            }

            application_menu
        }
    }

    unsafe fn create_dock_menu(
        &self,
        menu_items: Vec<MenuItem>,
        delegate: id,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> id {
        unsafe {
            let dock_menu: id = msg_send![class!(NSMenu), new];
            let _: () = msg_send![dock_menu, setDelegate: delegate];
            for item_config in menu_items {
                let menu_item = Self::create_menu_item(&item_config, delegate, actions, keymap);
                let _: () = msg_send![dock_menu, addItem: menu_item];
            }

            dock_menu
        }
    }

    unsafe fn create_menu_item(
        item: &MenuItem,
        delegate: id,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> id {
        static DEFAULT_CONTEXT: OnceLock<Vec<KeyContext>> = OnceLock::new();

        unsafe {
            match item {
                MenuItem::Separator => msg_send![class!(NSMenuItem), separatorItem],
                MenuItem::Action {
                    name,
                    action,
                    os_action,
                    checked,
                    disabled,
                } => {
                    // Note that this is intentionally using earlier bindings, whereas typically
                    // later ones take display precedence. See the discussion on
                    // https://github.com/zed-industries/zed/issues/23621
                    let keystrokes = keymap
                        .bindings_for_action(action.as_ref())
                        .find_or_first(|binding| {
                            binding.predicate().is_none_or(|predicate| {
                                predicate.eval(DEFAULT_CONTEXT.get_or_init(|| {
                                    let mut workspace_context = KeyContext::new_with_defaults();
                                    workspace_context.add("Workspace");
                                    let mut pane_context = KeyContext::new_with_defaults();
                                    pane_context.add("Pane");
                                    let mut editor_context = KeyContext::new_with_defaults();
                                    editor_context.add("Editor");

                                    pane_context.extend(&editor_context);
                                    workspace_context.extend(&pane_context);
                                    vec![workspace_context]
                                }))
                            })
                        })
                        .map(|binding| binding.keystrokes());

                    let selector = match os_action {
                        Some(gpui::OsAction::Cut) => sel!(cut:),
                        Some(gpui::OsAction::Copy) => sel!(copy:),
                        Some(gpui::OsAction::Paste) => sel!(paste:),
                        Some(gpui::OsAction::SelectAll) => sel!(selectAll:),
                        // "undo:" and "redo:" are always disabled in our case, as
                        // we don't have a NSTextView/NSTextField to enable them on.
                        Some(gpui::OsAction::Undo) => sel!(handleGPUIMenuItem:),
                        Some(gpui::OsAction::Redo) => sel!(handleGPUIMenuItem:),
                        None => sel!(handleGPUIMenuItem:),
                    };

                    let item;
                    if let Some(keystrokes) = keystrokes {
                        if keystrokes.len() == 1 {
                            let keystroke = &keystrokes[0];
                            let mut mask = NSEventModifierFlags(0);
                            for (modifier, flag) in &[
                                (
                                    keystroke.modifiers().platform,
                                    NSEventModifierFlags::Command,
                                ),
                                (keystroke.modifiers().control, NSEventModifierFlags::Control),
                                (keystroke.modifiers().alt, NSEventModifierFlags::Option),
                                (keystroke.modifiers().shift, NSEventModifierFlags::Shift),
                            ] {
                                if *modifier {
                                    mask |= *flag;
                                }
                            }

                            item = new_menu_item(
                                name,
                                selector,
                                key_to_native(keystroke.key()).as_ref(),
                            );
                            if Self::os_version() >= Version::new(12, 0, 0) {
                                let _: () = msg_send![item, setAllowsAutomaticKeyEquivalentLocalization: false];
                            }
                            let _: () = msg_send![item, setKeyEquivalentModifierMask: mask.0];
                        } else {
                            item = new_menu_item(name, selector, "");
                        }
                    } else {
                        item = new_menu_item(name, selector, "");
                    }

                    if *checked {
                        // NSControlStateValueOn
                        let _: () = msg_send![item, setState: 1 as NSInteger];
                    }
                    let _: () = msg_send![item, setEnabled: !*disabled];

                    let tag = actions.len() as NSInteger;
                    let _: () = msg_send![item, setTag: tag];
                    actions.push(action.boxed_clone());
                    item
                }
                MenuItem::Submenu(Menu {
                    name,
                    items,
                    disabled,
                }) => {
                    let item = new_autoreleased(class!(NSMenuItem));
                    let submenu = new_autoreleased(class!(NSMenu));
                    let _: () = msg_send![submenu, setDelegate: delegate];
                    for item in items {
                        let menu_item = Self::create_menu_item(item, delegate, actions, keymap);
                        let _: () = msg_send![submenu, addItem: menu_item];
                    }
                    let _: () = msg_send![item, setSubmenu: submenu];
                    let _: () = msg_send![item, setEnabled: !*disabled];
                    let _: () = msg_send![item, setTitle: ns_string(name)];
                    item
                }
                MenuItem::SystemMenu(OsMenu { name, menu_type }) => {
                    let item = new_autoreleased(class!(NSMenuItem));
                    let submenu = new_autoreleased(class!(NSMenu));
                    let _: () = msg_send![submenu, setDelegate: delegate];
                    let _: () = msg_send![item, setSubmenu: submenu];
                    let _: () = msg_send![item, setTitle: ns_string(name)];

                    match menu_type {
                        SystemMenuType::Services => {
                            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
                            let _: () = msg_send![app, setServicesMenu: item];
                        }
                    }

                    item
                }
            }
        }
    }

    fn os_version() -> Version {
        let version = unsafe {
            let process_info: id = msg_send![class!(NSProcessInfo), processInfo];
            let version: NSOperatingSystemVersion = msg_send![process_info, operatingSystemVersion];
            version
        };
        Version::new(
            version.majorVersion as u64,
            version.minorVersion as u64,
            version.patchVersion as u64,
        )
    }

    // Before the app finishes launching there is no delegate yet;
    // `did_finish_launching` registers the observers then.
    fn ensure_system_power_observers(&self) {
        if self.0.lock().system_power_observers_registered {
            return;
        }

        // The shared application must be created through `GPUIApplication`, or `run`
        // finds a plain `NSApplication` without the `platform` ivar; only the
        // delegate lookup goes through the typed binding.
        // SAFETY: `GPUIApplication` is registered on first use and `sharedApplication`
        // returns a live NSApplication instance.
        let delegate = unsafe {
            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
            (*(app as *const objc2_app_kit::NSApplication)).delegate()
        };
        if let Some(delegate) = delegate {
            register_system_power_observers(delegate.as_ref());
            self.0.lock().system_power_observers_registered = true;
        }
    }
}

impl Platform for MacPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.lock().background_executor.clone()
    }

    fn foreground_executor(&self) -> gpui::ForegroundExecutor {
        self.0.lock().foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.lock().text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        let mut state = self.0.lock();
        if state.headless {
            drop(state);
            on_finish_launching();
            unsafe { CFRunLoopRun() };
        } else {
            state.finish_launching = Some(on_finish_launching);
            drop(state);
        }

        unsafe {
            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
            // An accessory app must not register in the Dock during launch, so its
            // policy has to precede the run loop. `Regular` is applied in
            // `did_finish_launching` instead: setting it this early leaves the menu
            // bar of an unbundled app launched from a terminal unclickable.
            let policy = {
                let mut state = self.0.lock();
                state.application_created = true;
                state.activation_policy
            };
            if policy == ActivationPolicy::Accessory {
                let _: bool = msg_send![app, setActivationPolicy: native_activation_policy(policy)];
            }
            let app_delegate: id = msg_send![GPUIApplicationDelegate::class(), new];
            let _: () = msg_send![app, setDelegate: app_delegate];

            platform_ivar(&*app).set(self);
            platform_ivar(&*app_delegate).set(self);

            let pool = autorelease_pool();
            let _: () = msg_send![app, run];
            drain(pool);

            platform_ivar(&*app).set(ptr::null());
            let app_delegate: id = msg_send![app, delegate];
            platform_ivar(&*app_delegate).set(ptr::null());
        }
        self.0.lock().application_created = false;
    }

    fn quit(&self) {
        // Quitting the app causes us to close windows, which invokes `Window::on_close` callbacks
        // synchronously before this method terminates. If we call `Platform::quit` while holding a
        // borrow of the app state (which most of the time we will do), we will end up
        // double-borrowing the app state in the `on_close` callbacks for our open windows. To solve
        // this, we make quitting the application asynchronous so that we aren't holding borrows to
        // the app state on the stack when we actually terminate the app.

        unsafe {
            DispatchQueue::main().exec_async_f(ptr::null_mut(), quit);
        }

        extern "C" fn quit(_: *mut c_void) {
            unsafe {
                let app = shared_application();
                let _: () = msg_send![app, terminate: nil];
            }
        }
    }

    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<std::ffi::OsString>) {
        use std::os::unix::process::CommandExt as _;

        let app_pid = std::process::id().to_string();
        let app_path = binary_path
            .or_else(|| {
                self.app_path()
                    .ok()
                    // When the app is not bundled, `app_path` returns the
                    // directory containing the executable. Disregard this
                    // and get the path to the executable itself.
                    .and_then(|path| (path.extension()?.to_str()? == "app").then_some(path))
            })
            .unwrap_or_else(|| std::env::current_exe().unwrap());

        // Wait until this process has exited and then re-open this path.
        let script = r#"
            while kill -0 $0 2> /dev/null; do
                sleep 0.1
            done
            app_path="$1"
            shift
            if (($# > 0)); then
                open "$app_path" --args "$@"
            else
                open "$app_path"
            fi
        "#;

        #[allow(
            clippy::disallowed_methods,
            reason = "We are restarting ourselves, using std command thus is fine"
        )]
        let restart_process = new_std_command("/bin/bash")
            .arg("-c")
            .arg(script)
            .arg(app_pid)
            .arg(app_path)
            .args(arguments)
            .process_group(0)
            .spawn();

        match restart_process {
            Ok(_) => self.quit(),
            Err(e) => log::error!("failed to spawn restart script: {:?}", e),
        }
    }

    fn activate(&self, ignoring_other_apps: bool) {
        unsafe {
            let app = shared_application();
            let _: () = msg_send![app, activateIgnoringOtherApps: ignoring_other_apps];
        }
    }

    fn set_activation_policy(&self, policy: ActivationPolicy) {
        let mut state = self.0.lock();
        state.activation_policy = policy;
        let should_apply = state.application_created && !state.headless;
        drop(state);
        if should_apply {
            unsafe {
                let app: id = msg_send![GPUIApplication::class(), sharedApplication];
                let _: bool = msg_send![app, setActivationPolicy: native_activation_policy(policy)];
            }
        }
    }

    fn set_initial_windowing(&self, request: WindowingRequest) {
        self.set_activation_policy(activation_policy_for(&request));
    }

    fn request_windowing(&self, request: WindowingRequest) -> Task<Result<()>> {
        let state = self.0.lock();
        if state.headless {
            return Task::ready(Err(anyhow!(
                "a platform created headless has no application to switch windowing modes"
            )));
        }
        let policy = activation_policy_for(&request);
        if state.activation_policy == policy {
            return Task::ready(Err(match request {
                WindowingRequest::Headless => anyhow!("already headless"),
                WindowingRequest::Windowed(_) => anyhow!("already windowed"),
            }));
        }
        drop(state);
        self.set_activation_policy(policy);
        Task::ready(Ok(()))
    }

    fn graphical_environment(&self) -> Option<GraphicalEnvironment> {
        let state = self.0.lock();
        (!state.headless && state.activation_policy == ActivationPolicy::Regular)
            .then(GraphicalEnvironment::detect)
    }

    fn hide(&self) {
        unsafe {
            let app = shared_application();
            let _: () = msg_send![app, hide: nil];
        }
    }

    fn hide_other_apps(&self) {
        unsafe {
            let app = shared_application();
            let _: () = msg_send![app, hideOtherApplications: nil];
        }
    }

    fn unhide_other_apps(&self) {
        unsafe {
            let app = shared_application();
            let _: () = msg_send![app, unhideAllApplications: nil];
        }
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(MacDisplay::primary()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        MacDisplay::all()
            .map(|screen| Rc::new(screen) as Rc<_>)
            .collect()
    }

    fn on_displays_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().on_displays_changed = Some(callback);
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        MacWindow::active_window()
    }

    // Returns the windows ordered front-to-back, meaning that the active
    // window is the first one in the returned vec.
    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        Some(MacWindow::ordered_windows())
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        // Native popups are not implemented on macOS yet. Rejecting lets callers fall back to
        // gpui's in-window popovers.
        if let WindowKind::AnchoredPopup(_) = options.kind {
            return Err(PopupNotSupportedError.into());
        }

        let (cursor_visible, foreground_executor, background_executor, renderer_context) = {
            let guard = self.0.lock();
            (
                guard.cursor_visible.clone(),
                guard.foreground_executor.clone(),
                guard.background_executor.clone(),
                guard.renderer_context.clone(),
            )
        };

        Ok(Box::new(MacWindow::open(
            handle,
            options,
            cursor_visible,
            foreground_executor,
            background_executor,
            renderer_context,
            self.1,
        )))
    }

    fn window_appearance(&self) -> WindowAppearance {
        unsafe {
            let app = shared_application();
            let appearance: id = msg_send![app, effectiveAppearance];
            crate::window_appearance::window_appearance_from_native(appearance)
        }
    }

    fn set_window_appearance(&self, appearance: Option<WindowAppearance>) {
        unsafe {
            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
            // `None` clears the override by setting a nil appearance, so the app
            // falls back to tracking the system-wide light/dark setting.
            let ns_appearance: id = match appearance {
                None => nil,
                Some(appearance) => {
                    let name: id = match appearance {
                        WindowAppearance::Light => NSAppearanceNameAqua,
                        WindowAppearance::Dark => NSAppearanceNameDarkAqua,
                        WindowAppearance::VibrantLight => NSAppearanceNameVibrantLight,
                        WindowAppearance::VibrantDark => NSAppearanceNameVibrantDark,
                    };
                    msg_send![class!(NSAppearance), appearanceNamed: name]
                }
            };
            let _: () = msg_send![app, setAppearance: ns_appearance];
        }
    }

    fn open_url(&self, url: &str) {
        unsafe {
            let ns_url: id = msg_send![class!(NSURL), URLWithString: ns_string(url)];
            if ns_url.is_null() {
                log::error!("Failed to create NSURL from string: {}", url);
                return;
            }
            let url = ns_url;
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let _: bool = msg_send![workspace, openURL: url];
        }
    }

    fn register_url_scheme(&self, scheme: &str) -> Task<anyhow::Result<()>> {
        use objc2_foundation::{NSBundle, NSError, NSString};

        // API only available post Monterey
        // https://developer.apple.com/documentation/appkit/nsworkspace/3753004-setdefaultapplicationaturl
        let (done_tx, done_rx) = oneshot::channel();
        if Self::os_version() < Version::new(12, 0, 0) {
            return Task::ready(Err(anyhow!(
                "macOS 12.0 or later is required to register URL schemes"
            )));
        }

        let Some(bundle_id) = NSBundle::mainBundle().bundleIdentifier() else {
            return Task::ready(Err(anyhow!("Can only register URL scheme in bundled apps")));
        };

        let workspace = NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.URLForApplicationWithBundleIdentifier(&bundle_id) else {
            return Task::ready(Err(anyhow!(
                "Cannot register URL scheme until app is installed"
            )));
        };

        let scheme = NSString::from_str(scheme);

        let done_tx = Cell::new(Some(done_tx));
        let handler = RcBlock::new(move |error: *mut NSError| {
            let result = if let Some(error) = unsafe { error.as_ref() } {
                Err(anyhow!(
                    "Failed to register: {}",
                    error.localizedDescription()
                ))
            } else {
                Ok(())
            };

            if let Some(done_tx) = done_tx.take() {
                _ = done_tx.send(result);
            }
        });

        workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
            &app,
            &scheme,
            Some(&handler),
        );

        self.background_executor().spawn(async { done_rx.await? })
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.lock().open_urls = Some(callback);
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        use objc2_foundation::NSString;

        let marker = self.1;
        let (done_tx, done_rx) = oneshot::channel();
        self.foreground_executor()
            .spawn(async move {
                let panel = NSOpenPanel::openPanel(marker);
                panel.setCanChooseDirectories(options.directories);
                panel.setCanChooseFiles(options.files);
                panel.setAllowsMultipleSelection(options.multiple);

                panel.setCanCreateDirectories(true);
                panel.setResolvesAliases(false);

                let done_tx = Cell::new(Some(done_tx));
                let handler = RcBlock::new({
                    let panel = panel.clone();
                    move |response: NSModalResponse| {
                        let Some(done_tx) = done_tx.take() else {
                            return;
                        };

                        let result = (response == NSModalResponseOK).then(|| {
                            panel
                                .URLs()
                                .iter()
                                .filter(|url| url.isFileURL())
                                .filter_map(|url| url.to_file_path())
                                .collect::<Vec<_>>()
                        });
                        _ = done_tx.send(Ok(result));
                    }
                });

                if let Some(prompt) = options.prompt {
                    panel.setPrompt(Some(&NSString::from_str(prompt.as_str())));
                }

                panel.beginWithCompletionHandler(&handler);
            })
            .detach();
        done_rx
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        use objc2_foundation::{NSString, NSURL};

        let url = NSURL::from_directory_path(directory);
        let suggested_name = suggested_name.map(NSString::from_str);
        let (done_tx, done_rx) = oneshot::channel();
        let marker = self.1;
        self.foreground_executor()
            .spawn(async move {
                let panel = NSSavePanel::savePanel(marker);
                panel.setDirectoryURL(url.as_deref());

                if let Some(suggested_name) = suggested_name {
                    panel.setNameFieldStringValue(&suggested_name);
                }

                let done_tx = Cell::new(Some(done_tx));
                let handler = RcBlock::new({
                    let panel = panel.clone();
                    move |response: NSModalResponse| {
                        let Some(done_tx) = done_tx.take() else {
                            return;
                        };

                        let result = if response == NSModalResponseOK {
                            panel
                                .URL()
                                .filter(|url| url.isFileURL())
                                .and_then(|url| url.to_file_path())
                                .map(|mut path| {
                                    let Some(filename) = path.file_name() else {
                                        return path;
                                    };
                                    let chunks = filename
                                        .as_bytes()
                                        .split(|&b| b == b'.')
                                        .collect::<Vec<_>>();

                                    // https://github.com/zed-industries/zed/issues/16969
                                    // Workaround a bug in macOS Sequoia that adds an extra file-extension
                                    // sometimes. e.g. `a.sql` becomes `a.sql.s` or `a.txtx` becomes `a.txtx.txt`
                                    //
                                    // This is conditional on OS version because I'd like to get rid of it, so that
                                    // you can manually create a file called `a.sql.s`. That said it seems better
                                    // to break that use-case than breaking `a.sql`.
                                    if let &[_, second, third] = chunks.as_slice()
                                        && second.starts_with(third)
                                        && Self::os_version() >= Version::new(15, 0, 0)
                                    {
                                        path.set_extension("");
                                    }

                                    path
                                })
                        } else {
                            None
                        };

                        _ = done_tx.send(Ok(result));
                    }
                });

                panel.beginWithCompletionHandler(&handler);
            })
            .detach();

        done_rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        true
    }

    fn reveal_path(&self, path: &Path) {
        unsafe {
            let path = path.to_path_buf();
            self.0
                .lock()
                .background_executor
                .spawn(async move {
                    let full_path = ns_string(path.to_str().unwrap_or(""));
                    let root_full_path = ns_string("");
                    let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
                    let _: bool = msg_send![
                        workspace,
                        selectFile: full_path,
                        inFileViewerRootedAtPath: root_full_path
                    ];
                })
                .detach();
        }
    }

    fn open_with_system(&self, path: &Path) {
        let path = path.to_owned();
        self.0
            .lock()
            .background_executor
            .spawn(async move {
                #[allow(
                    clippy::disallowed_methods,
                    reason = "running on a background thread, so blocking is fine"
                )]
                new_std_command("open")
                    .arg("--")
                    .arg(path)
                    .status()
                    .context("invoking open command")
                    .log_err();
            })
            .detach();
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.lock().quit = Some(callback);
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().reopen = Some(callback);
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().on_keyboard_layout_change = Some(callback);
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.0.lock().menu_command = Some(callback);
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().will_open_menu = Some(callback);
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.0.lock().validate_menu_command = Some(callback);
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        let observer = ThermalObserver::new(&self.foreground_executor(), callback);
        self.0.lock().thermal_observer = Some(observer);
    }

    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().on_system_sleep = Some(callback);
        self.ensure_system_power_observers();
    }

    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().on_system_wake = Some(callback);
        self.ensure_system_power_observers();
    }

    fn thermal_state(&self) -> ThermalState {
        thermal::thermal_state()
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        Task::ready(Ok(AppleActivity::begin(
            reason,
            NSActivityOptions::UserInitiated,
        )))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(MacKeyboardLayout::new())
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.0.lock().keyboard_mapper.clone()
    }

    fn app_path(&self) -> Result<PathBuf> {
        unsafe {
            let bundle: id = msg_send![class!(NSBundle), mainBundle];
            anyhow::ensure!(!bundle.is_null(), "app is not running inside a bundle");
            Ok(path_from_objc(msg_send![bundle, bundlePath]))
        }
    }

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        unsafe {
            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
            let mut state = self.0.lock();
            let actions = &mut state.menu_actions;
            let menu = self.create_menu_bar(&menus, msg_send![app, delegate], actions, keymap);
            drop(state);
            let _: () = msg_send![app, setMainMenu: menu];
        }
        self.0.lock().menus = Some(menus.into_iter().map(|menu| menu.owned()).collect());
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.0.lock().menus.clone()
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        unsafe {
            let app: id = msg_send![GPUIApplication::class(), sharedApplication];
            let mut state = self.0.lock();
            let actions = &mut state.menu_actions;
            let new = self.create_dock_menu(menu, msg_send![app, delegate], actions, keymap);
            if let Some(old) = state.dock_menu.replace(new) {
                CFRelease(old as _)
            }
        }
    }

    fn add_recent_document(&self, path: &Path) {
        if let Some(path_str) = path.to_str() {
            unsafe {
                let document_controller: id =
                    msg_send![class!(NSDocumentController), sharedDocumentController];
                let url: id = msg_send![class!(NSURL), fileURLWithPath: ns_string(path_str)];
                let _: () = msg_send![document_controller, noteNewRecentDocumentURL:url];
            }
        }
    }

    /// Match cursor style to one of the styles available
    /// in macOS's [NSCursor](https://developer.apple.com/documentation/appkit/nscursor).
    fn set_cursor_style(&self, style: CursorStyle) {
        unsafe {
            set_active_window_cursor_style(style);
        }
    }

    fn hide_cursor_until_mouse_moves(&self) {
        let cursor_visible = self.0.lock().cursor_visible.clone();
        if !cursor_visible.swap(false, Ordering::Relaxed) {
            return;
        }
        unsafe {
            let _: () = msg_send![class!(NSCursor), setHiddenUntilMouseMoves: true];
        }
    }

    fn is_cursor_visible(&self) -> bool {
        self.0.lock().cursor_visible.load(Ordering::Relaxed)
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        #[allow(non_upper_case_globals)]
        const NSScrollerStyleOverlay: NSInteger = 1;

        unsafe {
            let style: NSInteger = msg_send![class!(NSScroller), preferredScrollerStyle];
            style == NSScrollerStyleOverlay
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        let state = self.0.lock();
        state.general_pasteboard.read()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        let state = self.0.lock();
        state.general_pasteboard.write(item);
    }

    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        let state = self.0.lock();
        state.find_pasteboard.read()
    }

    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        let state = self.0.lock();
        state.find_pasteboard.write(item);
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        let url = url.to_string();
        let username = username.to_string();
        let password = password.to_vec();
        self.background_executor()
            .spawn(async move { Ok(keychain::write_credentials(&url, &username, &password)?) })
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        let url = url.to_string();
        self.background_executor()
            .spawn(async move { Ok(keychain::read_credentials(&url)?) })
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        let url = url.to_string();
        self.background_executor()
            .spawn(async move { Ok(keychain::delete_credentials(&url)?) })
    }
}

unsafe fn path_from_objc(path: id) -> PathBuf {
    let len: NSUInteger =
        unsafe { msg_send![path, lengthOfBytesUsingEncoding: NSUTF8StringEncoding] };
    let bytes: *const c_char = unsafe { msg_send![path, UTF8String] };
    let path = str::from_utf8(unsafe { slice::from_raw_parts(bytes.cast::<u8>(), len) }).unwrap();
    PathBuf::from(path)
}

/// Returns the platform ivar of a `GPUIApplication` or `GPUIApplicationDelegate`.
fn platform_ivar(object: &AnyObject) -> &Cell<*const MacPlatform> {
    if let Some(app) = object.downcast_ref::<GPUIApplication>() {
        &app.ivars().platform
    } else if let Some(delegate) = object.downcast_ref::<GPUIApplicationDelegate>() {
        &delegate.ivars().platform
    } else {
        panic!("{object:?} is not the GPUI application or its delegate")
    }
}

unsafe fn get_mac_platform(object: &AnyObject) -> &MacPlatform {
    let platform_ptr = platform_ivar(object).get();
    assert!(!platform_ptr.is_null());
    unsafe { &*platform_ptr }
}

fn will_finish_launching(_this: &AnyObject, _: id) {
    unsafe {
        let user_defaults: id = msg_send![class!(NSUserDefaults), standardUserDefaults];

        // The autofill heuristic controller causes slowdown and high CPU usage.
        // We don't know exactly why. This disables the full heuristic controller.
        //
        // Adapted from: https://github.com/ghostty-org/ghostty/pull/8625
        // Registered (in-memory) rather than set, so nothing is persisted to
        // `~/Library/Preferences`.
        let name = ns_string("NSAutoFillHeuristicControllerEnabled");
        let false_value: id = msg_send![class!(NSNumber), numberWithBool:false];
        let dict: id = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: false_value,
            forKey: name
        ];
        let _: () = msg_send![user_defaults, registerDefaults: dict];

        // No native window tabs. Disabling automatic tabbing before any window or
        // menu exists also keeps AppKit from injecting "Show Tab Bar", "Show All
        // Tabs", "Merge All Windows", etc. into the View and Window menus.
        let () = msg_send![class!(NSWindow), setAllowsAutomaticWindowTabbing: false];
    }
}

fn did_finish_launching(this: &AnyObject, _: id) {
    unsafe {
        let app: id = msg_send![GPUIApplication::class(), sharedApplication];
        let policy = get_mac_platform(this).0.lock().activation_policy;
        let _: bool = msg_send![app, setActivationPolicy: native_activation_policy(policy)];

        let notification_center: *mut AnyObject =
            msg_send![class!(NSNotificationCenter), defaultCenter];
        let name = ns_string("NSTextInputContextKeyboardSelectionDidChangeNotification");
        let _: () = msg_send![notification_center, addObserver: this,
            selector: sel!(onKeyboardLayoutChange:),
            name: name,
            object: nil
        ];

        // Posted when a display is connected, disconnected, rearranged, or
        // changes mode, which includes its refresh rate.
        let _: () = msg_send![notification_center, addObserver: this,
            selector: sel!(onScreenParametersChange:),
            name: ns_string("NSApplicationDidChangeScreenParametersNotification"),
            object: nil
        ];

        let observer = this;
        let platform = get_mac_platform(this);
        let callback = {
            let mut state = platform.0.lock();
            if (state.on_system_sleep.is_some() || state.on_system_wake.is_some())
                && !state.system_power_observers_registered
            {
                register_system_power_observers(observer);
                state.system_power_observers_registered = true;
            }
            state.finish_launching.take()
        };
        if let Some(callback) = callback {
            callback();
        }
    }
}

/// `observer` must be the app delegate, whose class declares `onSystemSleep:`
/// and `onSystemWake:` (see `GPUIApplicationDelegate`).
fn register_system_power_observers(observer: &AnyObject) {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: both selectors are declared on the delegate class with the
    // notification-handler signature these observers are invoked with.
    unsafe {
        center.addObserver_selector_name_object(
            observer,
            sel!(onSystemSleep:),
            Some(NSWorkspaceWillSleepNotification),
            None,
        );
        center.addObserver_selector_name_object(
            observer,
            sel!(onSystemWake:),
            Some(NSWorkspaceDidWakeNotification),
            None,
        );
    }
}

fn should_handle_reopen(this: &AnyObject, _: id, has_open_windows: bool) {
    if !has_open_windows {
        let platform = unsafe { get_mac_platform(this) };
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.reopen.take() {
            drop(lock);
            callback();
            platform.0.lock().reopen.get_or_insert(callback);
        }
    }
}

fn will_terminate(this: &AnyObject, _: id) {
    let platform = unsafe { get_mac_platform(this) };
    let mut lock = platform.0.lock();
    if let Some(mut callback) = lock.quit.take() {
        drop(lock);
        callback();
        platform.0.lock().quit.get_or_insert(callback);
    }
}

fn on_keyboard_layout_change(this: &AnyObject, _: id) {
    let platform = unsafe { get_mac_platform(this) };
    let mut lock = platform.0.lock();
    let keyboard_layout = MacKeyboardLayout::new();
    lock.keyboard_mapper = Rc::new(MacKeyboardMapper::new(keyboard_layout.id()));
    if let Some(mut callback) = lock.on_keyboard_layout_change.take() {
        drop(lock);
        callback();
        platform
            .0
            .lock()
            .on_keyboard_layout_change
            .get_or_insert(callback);
    }
}

fn on_screen_parameters_change(this: &AnyObject, _: id) {
    // Defer to the next run loop iteration to avoid re-entrant borrows of the App RefCell,
    // as NSNotificationCenter delivers this notification synchronously and it may fire while
    // the App is already borrowed (same pattern as quit() above).
    let platform = unsafe { get_mac_platform(this) };
    let platform_ptr = platform as *const MacPlatform as *mut c_void;
    unsafe {
        DispatchQueue::main().exec_async_f(platform_ptr, on_screen_parameters_change);
    }

    extern "C" fn on_screen_parameters_change(context: *mut c_void) {
        let platform = unsafe { &*(context as *const MacPlatform) };
        let callback = platform.0.lock().on_displays_changed.take();
        if let Some(mut callback) = callback {
            callback();
            platform
                .0
                .lock()
                .on_displays_changed
                .get_or_insert(callback);
        }
    }
}

// Deferred to the main queue like the wake and thermal notifications. The
// machine keeps running the main run loop for well over a frame after the
// observers return, so the block runs before it suspends.
fn on_system_sleep(this: &AnyObject, _: id) {
    // SAFETY: this is the registered app delegate carrying the platform ivar.
    let platform = unsafe { get_mac_platform(this) };
    let platform_ptr = platform as *const MacPlatform as *mut c_void;
    // SAFETY: platform lives for the process lifetime while callbacks are registered.
    unsafe {
        DispatchQueue::main().exec_async_f(platform_ptr, on_system_sleep);
    }

    extern "C" fn on_system_sleep(context: *mut c_void) {
        // SAFETY: context is the MacPlatform pointer queued above.
        let platform = unsafe { &*(context as *const MacPlatform) };
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.on_system_sleep.take() {
            drop(lock);
            callback();
            platform.0.lock().on_system_sleep.get_or_insert(callback);
        }
    }
}

fn on_system_wake(this: &AnyObject, _: id) {
    // SAFETY: this is the registered app delegate carrying the platform ivar.
    let platform = unsafe { get_mac_platform(this) };
    let platform_ptr = platform as *const MacPlatform as *mut c_void;
    // SAFETY: platform lives for the process lifetime while callbacks are registered.
    unsafe {
        DispatchQueue::main().exec_async_f(platform_ptr, on_system_wake);
    }

    extern "C" fn on_system_wake(context: *mut c_void) {
        // SAFETY: context is the MacPlatform pointer queued above.
        let platform = unsafe { &*(context as *const MacPlatform) };
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.on_system_wake.take() {
            drop(lock);
            callback();
            platform.0.lock().on_system_wake.get_or_insert(callback);
        }
    }
}

fn open_urls(this: &AnyObject, _: id, urls: id) {
    let urls = unsafe {
        array_iter(urls)
            .filter_map(|url| {
                let absolute_string: id = msg_send![url, absoluteString];
                let utf8: *const c_char = msg_send![absolute_string, UTF8String];
                match CStr::from_ptr(utf8).to_str() {
                    Ok(string) => Some(string.to_string()),
                    Err(err) => {
                        log::error!("error converting path to string: {}", err);
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
    };
    let platform = unsafe { get_mac_platform(this) };
    let mut lock = platform.0.lock();
    if let Some(mut callback) = lock.open_urls.take() {
        drop(lock);
        callback(urls);
        platform.0.lock().open_urls.get_or_insert(callback);
    }
}

fn handle_menu_item(this: &AnyObject, item: id) {
    unsafe {
        let platform = get_mac_platform(this);
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.menu_command.take() {
            let tag: NSInteger = msg_send![item, tag];
            let index = tag as usize;
            if let Some(action) = lock.menu_actions.get(index) {
                let action = action.boxed_clone();
                drop(lock);
                callback(&*action);
            }
            platform.0.lock().menu_command.get_or_insert(callback);
        }
    }
}

fn validate_menu_item(this: &AnyObject, item: id) -> bool {
    unsafe {
        let mut result = false;
        let platform = get_mac_platform(this);
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.validate_menu_command.take() {
            let tag: NSInteger = msg_send![item, tag];
            let index = tag as usize;
            if let Some(action) = lock.menu_actions.get(index) {
                let action = action.boxed_clone();
                drop(lock);
                result = callback(action.as_ref());
            }
            platform
                .0
                .lock()
                .validate_menu_command
                .get_or_insert(callback);
        }
        result
    }
}

fn menu_will_open(this: &AnyObject, _: id) {
    unsafe {
        let platform = get_mac_platform(this);
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.will_open_menu.take() {
            drop(lock);
            callback();
            platform.0.lock().will_open_menu.get_or_insert(callback);
        }
    }
}

fn handle_dock_menu(this: &AnyObject, _: id) -> id {
    unsafe {
        let platform = get_mac_platform(this);
        let state = platform.0.lock();
        if let Some(id) = state.dock_menu {
            id
        } else {
            nil
        }
    }
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub(super) fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut AnyObject;
    pub(super) fn TISCopyCurrentKeyboardInputSource() -> *mut AnyObject;
    pub(super) fn TISGetInputSourceProperty(
        inputSource: *mut AnyObject,
        propertyKey: *const c_void,
    ) -> *mut AnyObject;

    pub(super) fn UCKeyTranslate(
        keyLayoutPtr: *const ::std::os::raw::c_void,
        virtualKeyCode: u16,
        keyAction: u16,
        modifierKeyState: u32,
        keyboardType: u32,
        keyTranslateOptions: u32,
        deadKeyState: *mut u32,
        maxStringLength: usize,
        actualStringLength: *mut usize,
        unicodeString: *mut u16,
    ) -> u32;
    pub(super) fn LMGetKbdType() -> u16;
    pub(super) static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    pub(super) static kTISPropertyInputSourceID: CFStringRef;
    pub(super) static kTISPropertyLocalizedName: CFStringRef;
    pub(super) static kTISPropertyInputSourceIsASCIICapable: CFStringRef;
    pub(super) static kTISPropertyInputSourceType: CFStringRef;
    pub(super) static kTISTypeKeyboardInputMode: CFStringRef;
}
