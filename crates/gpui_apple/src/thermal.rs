use block2::RcBlock;
use futures::{StreamExt, channel::mpsc};
use gpui::{ForegroundExecutor, Task, ThermalState};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSProcessInfo,
    NSProcessInfoThermalState, NSProcessInfoThermalStateDidChangeNotification,
};

pub fn thermal_state() -> ThermalState {
    match NSProcessInfo::processInfo().thermalState() {
        NSProcessInfoThermalState::Fair => ThermalState::Fair,
        NSProcessInfoThermalState::Serious => ThermalState::Serious,
        NSProcessInfoThermalState::Critical => ThermalState::Critical,
        _ => ThermalState::Nominal,
    }
}

pub struct ThermalObserver {
    center: Retained<NSNotificationCenter>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    _task: Task<()>,
}

impl ThermalObserver {
    pub fn new(executor: &ForegroundExecutor, callback: Box<dyn FnMut()>) -> Self {
        Self::with_center(NSNotificationCenter::defaultCenter(), executor, callback)
    }

    fn with_center(
        center: Retained<NSNotificationCenter>,
        executor: &ForegroundExecutor,
        mut callback: Box<dyn FnMut()>,
    ) -> Self {
        let (sender, mut receiver) = mpsc::unbounded();
        let block = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            // NotificationCenter can call synchronously or off-thread. Never enter
            // application code here: GPUI may already be borrowed on the main thread.
            if sender.unbounded_send(()).is_err() {
                log::debug!("thermal observer stopped before notification delivery");
            }
        });
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSProcessInfoThermalStateDidChangeNotification),
                Some(&NSProcessInfo::processInfo()),
                None,
                &block,
            )
        };
        let task = executor.spawn(async move {
            while receiver.next().await.is_some() {
                callback();
            }
        });
        Self {
            center,
            observer,
            _task: task,
        }
    }
}

impl Drop for ThermalObserver {
    fn drop(&mut self) {
        unsafe {
            self.center.removeObserver(self.observer.as_ref());
        }
    }
}
