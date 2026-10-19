//! Ikon di system tray dengan menu cepat.

use pacu::ui::UiEvent;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub fn create(tx: Sender<UiEvent>, ctx: eframe::egui::Context) -> Option<TrayIcon> {
    let show = MenuItem::new("Tampilkan Pacu", true, None);
    let auto = MenuItem::new("Mode Game: otomatis", true, None);
    let on = MenuItem::new("Mode Game: nyalakan", true, None);
    let off = MenuItem::new("Mode Game: matikan", true, None);
    let quit = MenuItem::new("Keluar", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &auto,
        &on,
        &off,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .ok()?;

    let ids = (show.id().clone(), auto.id().clone(), on.id().clone(), off.id().clone(), quit.id().clone());
    let menu_tx = Mutex::new(tx.clone());
    let menu_ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let ev = if e.id == ids.0 {
            UiEvent::Show
        } else if e.id == ids.1 {
            UiEvent::GameMode(None)
        } else if e.id == ids.2 {
            UiEvent::GameMode(Some(true))
        } else if e.id == ids.3 {
            UiEvent::GameMode(Some(false))
        } else if e.id == ids.4 {
            UiEvent::Quit
        } else {
            return;
        };
        let _ = menu_tx.lock().unwrap().send(ev);
        menu_ctx.request_repaint();
    }));

    let icon_tx = Mutex::new(tx);
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = e {
            let _ = icon_tx.lock().unwrap().send(UiEvent::Show);
            ctx.request_repaint();
        }
    }));

    let icon = tray_icon::Icon::from_rgba(pacu::icon::icon_rgba(32), 32, 32).ok()?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Pacu Game Booster")
        .with_icon(icon)
        .build()
        .ok()
}
