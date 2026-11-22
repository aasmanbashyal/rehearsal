#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod asr;
mod credentials;
mod media;
mod models;
#[cfg(not(feature = "full-media"))]
mod native_audio;
mod server;
mod sessions;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", test))]
mod download;
#[cfg(windows)]
mod webview_api;
#[cfg(all(windows, not(feature = "full-media")))]
mod windows_media;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
enum UiEvent {
    Command(String),
    #[cfg(windows)]
    Resource(usize, webview_api::Reply),
}

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let worker = args.first().is_some_and(|arg| arg == "--asr-worker");
    let headless = args.first().is_some_and(|arg| arg == "--serve");
    let result = if worker {
        asr_worker(&args[1..])
    } else {
        start(headless)
    };
    if let Err(error) = result {
        eprintln!("{error:#}");
        #[cfg(windows)]
        if !worker && !headless {
            message("Rehearsal could not start", &format!("{error:#}"), 0x10);
        }
        #[cfg(target_os = "macos")]
        if !worker && !headless {
            macos::show_error("Rehearsal could not start", &format!("{error:#}"));
        }
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn asr_worker(args: &[std::ffi::OsString]) -> Result<()> {
    let [library, model, audio] = args else {
        bail!("The speech worker requires a library, model and PCM file.");
    };
    let transcript = asr::run(Path::new(library), Path::new(model), Path::new(audio))?;
    println!("{}", serde_json::to_string(&transcript)?);
    Ok(())
}

fn start(headless: bool) -> Result<()> {
    let root = if let Some(path) = std::env::var_os("CADENCE_HOME") {
        PathBuf::from(path)
    } else {
        data_directory()?
    };
    let exe = std::env::current_exe()?;
    let resources = exe
        .parent()
        .context("Could not find the app folder.")?
        .join("resources");
    let port = std::env::var("CADENCE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4318);
    let port = if !headless && port == 0 { 4318 } else { port };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    #[cfg(windows)]
    if !headless {
        let Some(data_lock) = lock_desktop(&root)? else {
            return Ok(());
        };
        let studio = runtime.block_on(server::open(
            root.clone(),
            &resources,
            server::Transport::WebView,
            true,
        ))?;
        return window(
            root,
            server::WEBVIEW_ORIGIN.to_owned(),
            runtime,
            studio,
            data_lock,
        );
    }
    let (url, service) = runtime.block_on(server::start(root.clone(), &resources, port, !headless))?;
    if headless {
        println!("{url}");
        return runtime.block_on(service)?.map_err(Into::into);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return window(root, url, runtime, service);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    bail!("The desktop window is available on Windows, Linux and macOS. Use --serve for browser testing.");
}

fn data_directory() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    return Ok(gtk::glib::user_data_dir().join("rehearse"));
    #[cfg(target_os = "macos")]
    return Ok(PathBuf::from(
        std::env::var_os("HOME").context("Could not find your home folder.")?,
    )
    .join("Library/Application Support/Rehearse"));
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Ok(if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(path).join("Rehearse")
    } else {
        std::env::current_dir()?.join(".cadence")
    })
}

#[cfg(windows)]
fn lock_desktop(root: &Path) -> Result<Option<std::fs::File>> {
    use std::fs::{OpenOptions, TryLockError};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    std::fs::create_dir_all(root)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("desktop.lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => {
            unsafe {
                let window = FindWindowW(std::ptr::null(), wide("Rehearsal").as_ptr());
                if !window.is_null() {
                    if IsIconic(window) != 0 {
                        ShowWindow(window, SW_RESTORE);
                    }
                    SetForegroundWindow(window);
                }
            }
            Ok(None)
        }
        Err(TryLockError::Error(err)) => Err(err.into()),
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn window(
    root: PathBuf,
    url: String,
    runtime: tokio::runtime::Runtime,
    #[cfg(windows)] studio: std::sync::Arc<server::Studio>,
    #[cfg(windows)] data_lock: std::fs::File,
    #[cfg(not(windows))] service: tokio::task::JoinHandle<std::io::Result<()>>,
) -> Result<()> {
    use tao::{
        dpi::LogicalSize,
        event::{Event, StartCause, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::WindowBuilder,
    };
    use wry::{NewWindowResponse, PermissionKind, PermissionResponse, WebContext, WebViewBuilder};
    #[cfg(target_os = "macos")]
    if let Some(found) = server::loaded_webkit() {
        let (minimum, safari) = server::MINIMUM_WEBKIT;
        if found < minimum {
            bail!("Rehearsal needs Safari {safari} or later. Install the latest macOS or Safari update from System Settings > General > Software Update, then open Rehearsal again.");
        }
    }
    #[cfg(target_os = "linux")]
    use {
        tao::platform::unix::WindowExtUnix,
        webkit2gtk::{SettingsExt, WebViewExt},
        wry::{WebViewBuilderExtUnix, WebViewExtUnix},
    };
    #[cfg(windows)]
    use {
        tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows},
        tao::window::Icon,
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings9,
        windows::core::Interface,
        wry::{WebViewBuilderExtWindows, WebViewExtWindows},
    };

    #[cfg(target_os = "linux")]
    gtk::init().context("Could not initialize the Linux desktop display.")?;

    std::fs::create_dir_all(root.join("webview"))?;
    let mut context = WebContext::new(Some(root.join("webview")));
    let event_loop = EventLoopBuilder::<UiEvent>::with_user_event().build();
    let builder = WindowBuilder::new()
        .with_title("Rehearsal")
        .with_inner_size(LogicalSize::new(1120.0, 820.0))
        .with_min_inner_size(LogicalSize::new(400.0, 500.0));
    #[cfg(windows)]
    let builder = builder
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_window_icon(Icon::from_resource(101, None).ok())
        .with_taskbar_icon(
            Icon::from_resource(101, Some(tao::dpi::PhysicalSize::new(48, 48))).ok(),
        );
    let window = builder.build(&event_loop)?;
    #[cfg(target_os = "macos")]
    macos::install_menu();
    #[cfg(target_os = "linux")]
    gtk::prelude::GtkWindowExt::set_icon_name(window.gtk_window(), Some("rehearse"));
    let proxy = event_loop.create_proxy();
    #[cfg(target_os = "macos")]
    {
        // Quit from the menu or Dock runs the close button's unsaved-take check.
        let quit = std::sync::Mutex::new(event_loop.create_proxy());
        macos::guard_termination(move || {
            let _ = quit
                .lock()
                .unwrap()
                .send_event(UiEvent::Command("window-close".into()));
        });
    }
    let navigation = url.clone();
    let ipc_origin = url.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_clipboard(true)
        .with_navigation_handler(move |page| studio_page(&navigation, &page))
        .with_permission_handler(|kind| match kind {
            // Only this bundled page can load here. Capture starts from Record.
            PermissionKind::Microphone | PermissionKind::Camera => PermissionResponse::Allow,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            PermissionKind::DisplayCapture => PermissionResponse::Allow,
            _ => PermissionResponse::Default,
        })
        .with_new_window_req_handler(|page, _| {
            if page.starts_with("https://") {
                open_external(&page);
            }
            NewWindowResponse::Deny
        })
        .with_ipc_handler(move |request| {
            let command = request.body().as_str();
            if studio_page(&ipc_origin, &request.uri().to_string())
                && (matches!(
                    command,
                    "close"
                        | "confirm-close"
                        | "choose-folder"
                        | "window-close"
                        | "window-minimize"
                        | "window-toggle-maximize"
                        | "window-drag"
                        | "window-state"
                ) || command.starts_with("appearance:"))
            {
                let _ = proxy.send_event(UiEvent::Command(request.into_body()));
            }
        });
    #[cfg(windows)]
    let webview = builder
        .with_additional_browser_args("--disable-background-timer-throttling")
        .with_initialization_script_for_main_only("window.rehearsalWindow = true;", true)
        .build(&window)
        .context("Windows could not open the WebView2 runtime.")?;
    #[cfg(windows)]
    unsafe {
        // Use WebView2's native caption hit testing for the open space in the header.
        // Set it before navigation so it takes effect on the first page.
        if let Ok(settings) = webview
            .controller()
            .CoreWebView2()
            .and_then(|view| view.Settings())
            .and_then(|settings| settings.cast::<ICoreWebView2Settings9>())
        {
            let _ = settings.SetIsNonClientRegionSupportEnabled(true);
        }
    }
    #[cfg(target_os = "linux")]
    let webview = builder
        .build_gtk(
            window
                .default_vbox()
                .context("GTK window has no content area.")?,
        )
        .context("Linux could not open the WebKitGTK runtime.")?;
    #[cfg(target_os = "macos")]
    let webview = macos::with_downloads(builder)
        .build(&window)
        .context("macOS could not open its web view.")?;
    #[cfg(target_os = "linux")]
    if let Some(settings) = webview.webview().settings() {
        settings.set_enable_media_stream(true);
    }
    #[cfg(windows)]
    let bridge = webview_api::Bridge::install(
        &webview,
        runtime.handle(),
        studio,
        event_loop.create_proxy(),
    )
    .context("Could not connect the window to Rehearsal.")?;
    webview.load_url(&url)?;
    // The page answers the unsaved-take check over IPC. If it has not answered
    // by this deadline (it failed to load or its process exited), ask natively,
    // so neither the window nor a pending Quit or logout waits forever.
    const CLOSE_REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    let mut closing = false;
    let mut close_deadline: Option<std::time::Instant> = None;
    #[cfg(windows)]
    let mut maximized = window.is_maximized();
    event_loop.run(move |event, _, flow| {
        let _keep_alive = (&runtime, &context, &window);
        #[cfg(windows)]
        let _keep_data_lock = &data_lock;
        *flow = close_deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
        if matches!(&event, Event::NewEvents(StartCause::ResumeTimeReached { .. }))
            && closing && close_deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            closing = false;
            close_deadline = None;
            *flow = ControlFlow::Wait;
            if confirm_unresponsive_close(&window) {
                #[cfg(not(windows))]
                service.abort();
                #[cfg(target_os = "macos")]
                macos::finish_termination(true);
                *flow = ControlFlow::Exit;
            } else {
                #[cfg(target_os = "macos")]
                macos::finish_termination(false);
            }
            return;
        }
        if matches!(&event, Event::WindowEvent { event: WindowEvent::CloseRequested, .. })
            || matches!(&event, Event::UserEvent(UiEvent::Command(command)) if command == "window-close") {
            if !closing {
                closing = true;
                if webview.evaluate_script(
                    "window.ipc.postMessage(window.dispatchEvent(new Event('beforeunload', { cancelable: true })) ? 'close' : 'confirm-close');",
                ).is_err() {
                    #[cfg(not(windows))]
                    service.abort();
                    #[cfg(target_os = "macos")]
                    macos::finish_termination(true);
                    *flow = ControlFlow::Exit;
                } else {
                    let deadline = std::time::Instant::now() + CLOSE_REPLY_TIMEOUT;
                    close_deadline = Some(deadline);
                    *flow = ControlFlow::WaitUntil(deadline);
                }
            }
            return;
        }
        match event {
            Event::UserEvent(UiEvent::Command(choice)) if closing && matches!(choice.as_str(), "close" | "confirm-close") => {
                closing = false;
                close_deadline = None;
                *flow = ControlFlow::Wait;
                if choice == "close" || confirm_close(&window) {
                    #[cfg(not(windows))]
                    service.abort();
                    #[cfg(target_os = "macos")]
                    macos::finish_termination(true);
                    *flow = ControlFlow::Exit;
                } else {
                    // Cancel keeps the take recording, including after Quit.
                    #[cfg(target_os = "macos")]
                    macos::finish_termination(false);
                }
            }
            Event::UserEvent(UiEvent::Command(command)) if command == "choose-folder" => {
                let selected = choose_folder(&window);
                let path = serde_json::to_string(&selected.as_ref().map(|path| path.to_string_lossy())).unwrap();
                let _ = webview.evaluate_script(&format!("window.dispatchEvent(new CustomEvent('rehearse-folder', {{ detail: {path} }}))"));
            }
            #[cfg(windows)]
            Event::UserEvent(UiEvent::Command(command)) if command.starts_with("appearance:") => {
                apply_window_appearance(&window, &command);
            }
            #[cfg(windows)]
            Event::UserEvent(UiEvent::Command(command)) => match command.as_str() {
                "window-minimize" => window.set_minimized(true),
                "window-toggle-maximize" => window.set_maximized(!window.is_maximized()),
                "window-drag" => { let _ = window.drag_window(); }
                "window-state" => { let _ = webview.evaluate_script(&window_state(window.is_maximized())); }
                _ => {}
            },
            #[cfg(windows)]
            Event::UserEvent(UiEvent::Resource(id, reply)) => bridge.finish(id, reply),
            #[cfg(windows)]
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                let next = window.is_maximized();
                if maximized != next {
                    maximized = next;
                    let _ = webview.evaluate_script(&window_state(maximized));
                }
            }
            _ => {}
        }
    })
}

#[cfg(windows)]
fn window_state(maximized: bool) -> String {
    format!(
        "window.dispatchEvent(new CustomEvent('rehearsal-window-state', {{ detail: {maximized} }}))"
    )
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn studio_page(origin: &str, page: &str) -> bool {
    page.strip_prefix(origin)
        .is_some_and(|rest| matches!(rest.split('#').next(), Some("" | "/" | "/index.html")))
}

#[cfg(windows)]
fn apply_window_appearance(window: &tao::window::Window, command: &str) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    };
    let mut parts = command.split(':');
    let color = |hex: &str| -> Option<u32> {
        let hex = hex.strip_prefix('#')?;
        (hex.len() == 6).then_some(())?;
        Some(u32::from_str_radix(hex, 16).ok()?.swap_bytes() >> 8)
    };
    let (Some("appearance"), Some(theme @ ("light" | "dark")), Some(border), None) = (
        parts.next(),
        parts.next(),
        parts.next().and_then(color),
        parts.next(),
    ) else {
        return;
    };
    for (attribute, value) in [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(theme == "dark")),
        (DWMWA_BORDER_COLOR, border),
    ] {
        unsafe {
            DwmSetWindowAttribute(
                window.hwnd() as _,
                attribute as u32,
                (&value as *const u32).cast(),
                4,
            );
        }
    }
}

#[cfg(windows)]
fn choose_folder(window: &tao::window::Window) -> Option<PathBuf> {
    use tao::platform::windows::WindowExtWindows;
    use windows::{
        Win32::{
            Foundation::HWND,
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
            UI::Shell::{
                FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
                IFileOpenDialog, SIGDN_FILESYSPATH,
            },
        },
        core::w,
    };
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(
                dialog.GetOptions().ok()?
                    | FOS_PICKFOLDERS
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST,
            )
            .ok()?;
        dialog.SetTitle(w!("Choose a recording folder")).ok()?;
        dialog.Show(Some(HWND(window.hwnd() as _))).ok()?;
        let name = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0.cast()));
        path
    }
}

#[cfg(target_os = "linux")]
fn choose_folder(window: &tao::window::Window) -> Option<PathBuf> {
    use gtk::prelude::{DialogExt, FileChooserExt, WidgetExt};
    use tao::platform::unix::WindowExtUnix;
    let dialog = gtk::FileChooserDialog::with_buttons(
        Some("Choose a recording folder"),
        Some(window.gtk_window()),
        gtk::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Choose", gtk::ResponseType::Accept),
        ],
    );
    let selected = (dialog.run() == gtk::ResponseType::Accept)
        .then(|| dialog.filename())
        .flatten();
    dialog.hide();
    selected
}

#[cfg(windows)]
fn confirm_close(_window: &tao::window::Window) -> bool {
    message(
        "Close Rehearsal?",
        "A recording is in progress or has not been saved. Close Rehearsal?",
        0x24,
    ) == 6
}

#[cfg(target_os = "linux")]
fn confirm_close(window: &tao::window::Window) -> bool {
    use gtk::prelude::{DialogExt, WidgetExt};
    use tao::platform::unix::WindowExtUnix;

    let dialog = gtk::MessageDialog::new(
        Some(window.gtk_window()),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Question,
        gtk::ButtonsType::YesNo,
        "A recording is in progress or has not been saved. Close Rehearsal?",
    );
    dialog.set_default_response(gtk::ResponseType::No);
    let response = dialog.run();
    dialog.hide();
    response == gtk::ResponseType::Yes
}

#[cfg(target_os = "linux")]
fn open_external(url: &str) {
    let _ = gtk::gio::AppInfo::launch_default_for_uri(url, None::<&gtk::gio::AppLaunchContext>);
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn message(title: &str, text: &str, kind: u32) -> i32 {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide(title).as_ptr(),
            kind,
        )
    }
}

#[cfg(windows)]
fn open_external(url: &str) {
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}

#[cfg(target_os = "macos")]
fn choose_folder(_window: &tao::window::Window) -> Option<PathBuf> {
    macos::choose_folder()
}

#[cfg(target_os = "macos")]
fn confirm_unresponsive_close(_window: &tao::window::Window) -> bool {
    macos::confirm_unresponsive_close()
}

#[cfg(any(windows, target_os = "linux"))]
fn confirm_unresponsive_close(window: &tao::window::Window) -> bool {
    confirm_close(window)
}

#[cfg(target_os = "macos")]
fn confirm_close(_window: &tao::window::Window) -> bool {
    macos::confirm_close()
}

#[cfg(target_os = "macos")]
fn open_external(url: &str) {
    macos::open_external(url);
}
