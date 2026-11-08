//! Menu bar item (macOS): link state at a glance and quick controls, so the
//! app can keep streaming with its window closed.
//!
//! gpui has no public API for this, so it talks to AppKit directly. Menu
//! clicks are queued on a channel and handled on the next gpui tick.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gpui::{App, Entity};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSString;

use crate::bridge::{Bridge, Health};

const TICK: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
enum Command {
    ToggleSending,
    ShowWindow,
    Quit,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and MenuTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TX16SBridgeMenuTarget"]
    #[ivars = Sender<Command>]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(toggleSending:))]
        fn toggle_sending(&self, _sender: Option<&AnyObject>) {
            let _ = self.ivars().send(Command::ToggleSending);
        }

        #[unsafe(method(showWindow:))]
        fn show_window(&self, _sender: Option<&AnyObject>) {
            let _ = self.ivars().send(Command::ShowWindow);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            let _ = self.ivars().send(Command::Quit);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, commands: Sender<Command>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(commands);
        unsafe { msg_send![super(this), init] }
    }
}

struct StatusBar {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    radio: Retained<NSMenuItem>,
    network: Retained<NSMenuItem>,
    receiver: Retained<NSMenuItem>,
    toggle: Retained<NSMenuItem>,
    _target: Retained<MenuTarget>,
    commands: Receiver<Command>,
    shown: RefCell<Option<(Health, [String; 4])>>,
}

impl StatusBar {
    fn new(mtm: MainThreadMarker) -> Self {
        let (sender, commands) = channel();
        let target = MenuTarget::new(mtm, sender);
        let menu = NSMenu::new(mtm);
        let info = |title: &str| menu_item(mtm, &menu, title, None, &target);
        let radio = info("Radio");
        let network = info("Envoi");
        let receiver = info("PC");
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let toggle = menu_item(mtm, &menu, "Démarrer l'envoi", Some(sel!(toggleSending:)), &target);
        menu_item(mtm, &menu, "Ouvrir TX16S Bridge…", Some(sel!(showWindow:)), &target);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu_item(mtm, &menu, "Quitter TX16S Bridge", Some(sel!(quit:)), &target);

        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        item.setMenu(Some(&menu));
        Self {
            mtm,
            item,
            radio,
            network,
            receiver,
            toggle,
            _target: target,
            commands,
            shown: RefCell::new(None),
        }
    }

    /// Refreshes the icon and menu, touching AppKit only when something changed.
    fn show(&self, bridge: &Bridge) {
        let health = bridge.health();
        let lines = [
            format!("Radio : {}", bridge.radio_status().0),
            format!("Envoi : {}", bridge.network_status().0),
            format!("PC : {}", bridge.receiver_status().0),
            if bridge.config.sending { "Arrêter l'envoi" } else { "Démarrer l'envoi" }.to_owned(),
        ];
        let mut shown = self.shown.borrow_mut();
        let previous = shown.as_ref();
        if previous.is_none_or(|(h, _)| *h != health)
            && let Some(button) = self.item.button(self.mtm)
        {
            button.setImage(icon(health).as_deref());
            button.setToolTip(Some(&NSString::from_str(tooltip(health))));
        }
        for (i, item) in [&self.radio, &self.network, &self.receiver, &self.toggle].iter().enumerate()
        {
            if previous.is_none_or(|(_, l)| l[i] != lines[i]) {
                item.setTitle(&NSString::from_str(&lines[i]));
            }
        }
        *shown = Some((health, lines));
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    title: &str,
    action: Option<Sel>,
    target: &MenuTarget,
) -> Retained<NSMenuItem> {
    // SAFETY: every action selector is implemented by MenuTarget with the expected signature.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    };
    if action.is_some() {
        // SAFETY: the target outlives the menu, both being owned by StatusBar.
        unsafe { item.setTarget(Some(target)) };
    }
    menu.addItem(&item);
    item
}

fn icon(health: Health) -> Option<Retained<NSImage>> {
    let symbol = match health {
        Health::Streaming => "antenna.radiowaves.left.and.right",
        Health::Paused => "antenna.radiowaves.left.and.right.slash",
        Health::Problem => "exclamationmark.triangle",
    };
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        Some(&NSString::from_str(tooltip(health))),
    )?;
    // Template images follow the menu bar's light/dark appearance.
    image.setTemplate(true);
    Some(image)
}

fn tooltip(health: Health) -> &'static str {
    match health {
        Health::Streaming => "TX16S Bridge : envoi en cours",
        Health::Paused => "TX16S Bridge : en pause",
        Health::Problem => "TX16S Bridge : problème de liaison",
    }
}

/// Shows or hides the Dock icon: a menu bar app only needs it while its window is open.
pub fn set_dock_icon_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
}

pub fn install(bridge: Entity<Bridge>, show_window: impl Fn(&mut App) + 'static, cx: &mut App) {
    let mtm = MainThreadMarker::new().expect("the menu bar item must be created on the main thread");
    let status_bar = StatusBar::new(mtm);
    cx.spawn(async move |cx| {
        loop {
            let alive = cx.update(|cx| {
                while let Ok(command) = status_bar.commands.try_recv() {
                    match command {
                        Command::ToggleSending => {
                            bridge.update(cx, |bridge, cx| bridge.toggle_sending(cx));
                        }
                        Command::ShowWindow => show_window(cx),
                        Command::Quit => cx.quit(),
                    }
                }
                status_bar.show(bridge.read(cx));
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor().timer(TICK).await;
        }
    })
    .detach();
}
