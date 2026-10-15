//! Headless event loop for the Linux remote server. No display server or GPU is used.

use std::{
    cell::RefCell,
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread::{self, ThreadId},
    time::Duration,
};

use anyhow::{Result, anyhow, bail};
use futures::channel::oneshot;
use gpui::*;

pub(crate) struct HeadlessPlatform {
    background: BackgroundExecutor,
    foreground: ForegroundExecutor,
    sender: flume::Sender<MainEvent>,
    receiver: RefCell<Option<flume::Receiver<MainEvent>>>,
    quit_callback: RefCell<Option<Box<dyn FnMut() -> bool>>>,
}

enum MainEvent {
    Run(RunnableVariant),
    Quit,
}

struct Dispatcher {
    main_thread: ThreadId,
    main_sender: flume::Sender<MainEvent>,
    executor: Arc<smol::Executor<'static>>,
    _shutdown: flume::Sender<()>,
}

impl HeadlessPlatform {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        let (shutdown, stopped) = flume::unbounded::<()>();
        let executor = Arc::new(smol::Executor::new());
        let workers = thread::available_parallelism().map_or(2, |count| count.get().max(2));
        for i in 0..workers {
            let executor = executor.clone();
            let stopped = stopped.clone();
            thread::Builder::new()
                .name(format!("remote-worker-{i}"))
                .spawn(move || smol::block_on(executor.run(stopped.recv_async())))
                .expect("failed to spawn remote server worker");
        }
        let dispatcher = Arc::new(Dispatcher {
            main_thread: thread::current().id(),
            main_sender: sender.clone(),
            executor,
            _shutdown: shutdown,
        });
        Self {
            background: BackgroundExecutor::new(dispatcher.clone()),
            foreground: ForegroundExecutor::new(dispatcher),
            sender,
            receiver: RefCell::new(Some(receiver)),
            quit_callback: RefCell::new(None),
        }
    }
}

impl PlatformDispatcher for Dispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread
    }

    fn dispatch(&self, runnable: RunnableVariant, _priority: Priority) {
        self.executor
            .spawn(async move {
                runnable.run();
            })
            .detach();
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        if let Err(error) = self.main_sender.send(MainEvent::Run(runnable)) {
            // A local future must not be dropped on a worker during shutdown.
            std::mem::forget(error);
        }
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "the platform dispatcher implements executor timers"
    )]
    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        self.executor
            .spawn(async move {
                smol::Timer::after(duration).await;
                runnable.run();
            })
            .detach();
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        thread::spawn(f);
    }
}

impl Platform for HeadlessPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.background.clone()
    }
    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground.clone()
    }
    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        Arc::new(NoopTextSystem::new())
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        let receiver = self
            .receiver
            .borrow_mut()
            .take()
            .expect("event loop already started");
        on_finish_launching();
        while let Ok(event) = receiver.recv() {
            match event {
                MainEvent::Run(runnable) => {
                    runnable.run();
                }
                MainEvent::Quit => {
                    if self
                        .quit_callback
                        .borrow_mut()
                        .as_mut()
                        .is_none_or(|callback| callback())
                    {
                        break;
                    }
                }
            }
        }
    }

    fn quit(&self) {
        let _ = self.sender.send(MainEvent::Quit);
    }
    fn restart(&self, _binary: Option<PathBuf>, _arguments: Vec<OsString>) {
        self.quit();
    }
    fn activate(&self, _ignoring_other_apps: bool) {}
    fn hide(&self) {}
    fn hide_other_apps(&self) {}
    fn unhide_other_apps(&self) {}
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        Vec::new()
    }
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }
    fn on_displays_changed(&self, _callback: Box<dyn FnMut()>) {}
    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }
    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        _options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        bail!("the Linux remote server cannot open windows")
    }
    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::Dark
    }
    fn open_url(&self, _url: &str) {}
    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}
    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "URL schemes are unavailable on the remote server"
        )))
    }
    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }
    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }
    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }
    fn reveal_path(&self, _path: &Path) {}
    fn open_with_system(&self, _path: &Path) {}
    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        *self.quit_callback.borrow_mut() = Some(callback);
    }
    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}
    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}
    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}
    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}
    fn set_dock_menu(&self, _menus: Vec<MenuItem>, _keymap: &Keymap) {}
    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}
    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}
    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}
    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }
    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}
    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<ActivityGuard>> {
        Task::ready(Ok(ActivityGuard::noop()))
    }
    fn app_path(&self) -> Result<PathBuf> {
        Ok(std::env::current_exe()?)
    }
    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        Ok(self
            .app_path()?
            .parent()
            .ok_or_else(|| anyhow!("missing executable directory"))?
            .join(name))
    }
    fn set_cursor_style(&self, _style: CursorStyle) {}
    fn hide_cursor_until_mouse_moves(&self) {}
    fn is_cursor_visible(&self) -> bool {
        false
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        None
    }
    fn write_to_clipboard(&self, _item: ClipboardItem) {}
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        None
    }
    fn write_to_find_pasteboard(&self, _item: ClipboardItem) {}
    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "credential storage is unavailable on the remote server"
        )))
    }
    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }
    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(HeadlessKeyboardLayout)
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}

struct HeadlessKeyboardLayout;
impl PlatformKeyboardLayout for HeadlessKeyboardLayout {
    fn id(&self) -> &str {
        "headless"
    }
    fn name(&self) -> &str {
        "Headless"
    }
}
