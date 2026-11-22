//! AppKit pieces of the macOS desktop window: the menu bar, folder picker, save
//! panel, close confirmation and error alerts. These run on the main thread,
//! except `webkit_version`, which any thread may call.

use objc2::{
    MainThreadMarker, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
    sel,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSEventModifierFlags, NSMenu,
    NSMenuItem, NSModalResponseOK, NSOpenPanel, NSSavePanel,
};
use objc2_foundation::{NSBundle, NSString, NSURL};
use std::{
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

const APP: &str = "Rehearsal";

/// macOS routes Copy, Paste, Undo and Quit shortcuts through the menu bar, so
/// WKWebView text fields need these standard items to receive them.
pub(crate) fn install_menu() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);

    let item = |title: &str, action: Option<Sel>, key: &str| unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    let submenu = |title: &str, items: Vec<Retained<NSMenuItem>>| {
        let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(title));
        for entry in items {
            menu.addItem(&entry);
        }
        let holder = item(title, None, "");
        holder.setSubmenu(Some(&menu));
        bar.addItem(&holder);
        menu
    };
    let separator = || NSMenuItem::separatorItem(mtm);
    let hide_others = item("Hide Others", Some(sel!(hideOtherApplications:)), "h");
    hide_others.setKeyEquivalentModifierMask(
        NSEventModifierFlags::Command | NSEventModifierFlags::Option,
    );

    submenu(APP, vec![
        item(&format!("About {APP}"), Some(sel!(orderFrontStandardAboutPanel:)), ""),
        separator(),
        item(&format!("Hide {APP}"), Some(sel!(hide:)), "h"),
        hide_others,
        item("Show All", Some(sel!(unhideAllApplications:)), ""),
        separator(),
        // Quit goes through application termination, like the Dock's Quit,
        // so both reach the unsaved-take check in `guard_termination`.
        item(&format!("Quit {APP}"), Some(sel!(terminate:)), "q"),
    ]);
    let redo = item("Redo", Some(sel!(redo:)), "z");
    redo.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    submenu("Edit", vec![
        item("Undo", Some(sel!(undo:)), "z"),
        redo,
        separator(),
        item("Cut", Some(sel!(cut:)), "x"),
        item("Copy", Some(sel!(copy:)), "c"),
        item("Paste", Some(sel!(paste:)), "v"),
        item("Select All", Some(sel!(selectAll:)), "a"),
    ]);
    let full_screen = item("Enter Full Screen", Some(sel!(toggleFullScreen:)), "f");
    full_screen.setKeyEquivalentModifierMask(
        NSEventModifierFlags::Command | NSEventModifierFlags::Control,
    );
    submenu("View", vec![full_screen]);
    let window = submenu("Window", vec![
        item("Minimize", Some(sel!(performMiniaturize:)), "m"),
        item("Zoom", Some(sel!(performZoom:)), ""),
        separator(),
        item("Close", Some(sel!(performClose:)), "w"),
    ]);
    app.setMainMenu(Some(&bar));
    app.setWindowsMenu(Some(&window));
}

pub(crate) fn choose_folder() -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setCanCreateDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str("Choose a recording folder")));
    panel.setPrompt(Some(&NSString::from_str("Choose")));
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let url: Retained<NSURL> = panel.URL()?;
    url.to_file_path()
}

/// Asks where to save a download. `suggested` is the default destination in
/// Downloads; the panel starts there with the page's file name.
fn choose_save_path(suggested: &Path) -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSSavePanel::savePanel(mtm);
    panel.setCanCreateDirectories(true);
    if let Some(folder) = suggested.parent().and_then(NSURL::from_directory_path) {
        panel.setDirectoryURL(Some(&folder));
    }
    if let Some(name) = suggested.file_name() {
        panel.setNameFieldStringValue(&NSString::from_str(&name.to_string_lossy()));
    }
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    panel.URL()?.to_file_path()
}

pub(crate) fn with_downloads(builder: wry::WebViewBuilder<'_>) -> wry::WebViewBuilder<'_> {
    use crate::download::PendingExport;
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    // Wry returns the original URL on completion, including for blob downloads.
    // downloadFile creates a distinct URL for each click.
    let pending = Rc::new(RefCell::new(HashMap::new()));
    let started = Rc::clone(&pending);
    builder
        .with_download_started_handler(move |url, destination| {
            let Some(selected) = choose_save_path(destination) else { return false };
            match PendingExport::new(selected) {
                Ok(export) => {
                    *destination = export.staging_path();
                    started.borrow_mut().insert(url, export);
                    true
                }
                Err(error) => {
                    show_error("Download failed", &error.to_string());
                    false
                }
            }
        })
        .with_download_completed_handler(move |url, _, success| {
            let Some(export) = pending.borrow_mut().remove(&url) else { return };
            if !success {
                show_error("Download failed", &format!("Could not save {}.", export.destination.display()));
            } else if let Err(error) = export.finish() {
                show_error("Download failed", &format!("Could not save {}: {error}", export.destination.display()));
            }
        })
}

/// The `CFBundleVersion` of the WebKit this process loaded, such as
/// "19618.3.11.11.5". It comes from the bundle that defines `WKWebView`, so a
/// Safari update installed on an older macOS release is reflected here.
pub(crate) fn webkit_version() -> Option<String> {
    let class = AnyClass::get(c"WKWebView")?;
    // SAFETY: `class` is a loaded Objective-C class.
    let bundle = unsafe { NSBundle::bundleForClass(class) };
    let version = bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleVersion"))?;
    Some(version.downcast::<NSString>().ok()?.to_string())
}

fn alert(title: &str, text: &str, buttons: &[&str], style: NSAlertStyle) -> Option<isize> {
    let mtm = MainThreadMarker::new()?;
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    alert.setAlertStyle(style);
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    Some(alert.runModal())
}

pub(crate) fn confirm_close() -> bool {
    confirm(&format!("Close {APP}?"), "A recording is in progress or has not been saved.")
}

/// Used when the page did not answer the unsaved-take check in time, so the
/// recording state is unknown.
pub(crate) fn confirm_unresponsive_close() -> bool {
    confirm(
        &format!("{APP} is not responding"),
        &format!("{APP} could not check for a recording in progress. Closing now may lose a take that has not been saved."),
    )
}

fn confirm(title: &str, text: &str) -> bool {
    alert(title, text, &["Cancel", "Close"], NSAlertStyle::Warning)
        .is_some_and(|choice| choice == NSAlertFirstButtonReturn + 1)
}

pub(crate) fn show_error(title: &str, text: &str) {
    let _ = alert(title, text, &["OK"], NSAlertStyle::Critical);
}

pub(crate) fn open_external(url: &str) {
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}

// NSApplicationTerminateReply values.
const TERMINATE_NOW: usize = 1;
const TERMINATE_LATER: usize = 2;

static CLOSE_REQUEST: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static TERMINATION_PENDING: AtomicBool = AtomicBool::new(false);

/// Routes Quit from the app menu, the Dock, logout and Apple Events through the
/// window's unsaved-take check. Tao's app delegate does not answer
/// `applicationShouldTerminate:`, so AppKit would otherwise terminate at once
/// and only report `applicationWillTerminate:` once it is too late to cancel.
/// `request_close` must start the same flow as the window's close button;
/// that flow then calls `finish_termination` with the user's answer.
pub(crate) fn guard_termination(request_close: impl Fn() + Send + Sync + 'static) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if CLOSE_REQUEST.set(Box::new(request_close)).is_err() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&app, delegate] };
    let Some(delegate) = delegate else { return };
    extern "C" fn should_terminate(_: &AnyObject, _: Sel, _: *mut AnyObject) -> usize {
        let Some(request) = CLOSE_REQUEST.get() else {
            return TERMINATE_NOW;
        };
        // A second Quit while the check is running waits for the same answer.
        if !TERMINATION_PENDING.swap(true, Ordering::SeqCst) {
            request();
        }
        TERMINATE_LATER
    }
    let imp: extern "C" fn(&AnyObject, Sel, *mut AnyObject) -> usize = should_terminate;
    // SAFETY: the signature matches the "Q@:@" type encoding of
    // -[NSApplicationDelegate applicationShouldTerminate:] on 64-bit macOS.
    unsafe {
        let class = delegate.class() as *const _ as *mut objc2::runtime::AnyClass;
        let added = objc2::ffi::class_addMethod(
            class,
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<extern "C" fn(&AnyObject, Sel, *mut AnyObject) -> usize, Imp>(imp),
            c"Q@:@".as_ptr(),
        );
        if added == Bool::NO {
            eprintln!("Quit will skip the unsaved-take check: the app delegate already handles it.");
        }
    }
}

/// Answers a pending Quit: `true` lets AppKit terminate, `false` cancels it and
/// leaves any recording running. Does nothing when no Quit is waiting.
pub(crate) fn finish_termination(allow: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if TERMINATION_PENDING.swap(false, Ordering::SeqCst) {
        let app = NSApplication::sharedApplication(mtm);
        unsafe {
            let () = msg_send![&app, replyToApplicationShouldTerminate: Bool::new(allow)];
        }
    }
}
