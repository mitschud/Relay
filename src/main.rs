mod activity;
mod adaptive;
mod agent;
mod attachments;
mod backend;
mod branding;
mod chat_fade;
mod clipboard_helper;
mod codex_helper;
mod compaction;
mod composer;
mod config;
mod controls;
mod drawer;
mod engine;
mod fingerprint;
mod footer;
mod http;
mod instance;
mod local_model;
mod markdown;
mod memory;
mod metrics;
mod model;
mod picker;
mod profiles;
mod scroll_pill;
mod skills;
mod strata_helper;
mod sudo;
mod supervisor;
mod telemetry;
mod tray;
mod typography;
mod transitions;
mod ui;
mod util;
mod web;

/// One installed binary; legacy helper names are dispatched by argv[0] and all
/// internal helpers by a hidden subcommand.
fn dispatch(args: &[String]) -> bool {
    let name = std::path::Path::new(&args[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let legacy = match name {
        "relay-engine" => Some("__engine"),
        "relay-strata" => Some("__strata"),
        "relay-codex" => Some("__codex"),
        "relay-skills" => Some("__skills"),
        "relay-clipboard" => Some("__clipboard"),
        "relay-sudo" => Some("sudo"),
        "relay-askpass" => Some("__askpass"),
        "relay-local-model" => Some("__local-model"),
        "relay-web" => Some("__web-mcp"),
        _ => None,
    };
    let (command, offset) = match legacy {
        Some(command) => (command, 1usize),
        None => match args.get(1).map(String::as_str) {
            Some("web-mcp") => ("__web-mcp", 2),
            Some("local-model") => ("__local-model", 2),
            Some(
                command @ ("__engine" | "__strata" | "__codex" | "__skills" | "__clipboard"
                | "__sudo-session" | "__askpass" | "__local-model" | "__web-mcp" | "sudo"),
            ) => (command, 2),
            _ => return false,
        },
    };
    let rest = &args[offset..];
    match command {
        "__engine" => supervisor::main(rest),
        "__strata" => strata_helper::main(rest),
        "__codex" => codex_helper::main(rest),
        "__skills" => skills::helper_main(rest),
        "__clipboard" => clipboard_helper::main(),
        "__sudo-session" => sudo::session_main(rest),
        "__askpass" => sudo::askpass_main(),
        "__local-model" => local_model::main(),
        "__web-mcp" => web::main(),
        "sudo" => sudo::client_main(rest),
        _ => return false,
    }
    true
}

pub(crate) fn apply_software_rendering() {
    // Set before any threads/GL initialization. Applies only to this frontend.
    if std::env::var("RELAY_HARDWARE_RENDERING").as_deref() != Ok("1") {
        unsafe {
            for (key, value) in [
                ("__GLX_VENDOR_LIBRARY_NAME", "mesa"),
                ("LIBGL_ALWAYS_SOFTWARE", "1"),
                ("GALLIUM_DRIVER", "llvmpipe"),
                ("LP_NUM_THREADS", "2"),
                ("RELAY_SOFTWARE_FRONTEND", "1"),
            ] {
                std::env::set_var(key, value);
            }
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if dispatch(&args) {
        return Ok(());
    }
    apply_software_rendering();
    if let Err(error) = sudo::ensure_helpers() {
        eprintln!("Could not install Relay helper links: {error}");
    }
    let demo = args.iter().any(|s| s == "--demo");
    let instance = match instance::Instance::acquire(demo) {
        Ok(Some(x)) => x,
        Ok(None) => return Ok(()),
        Err(e) => {
            eprintln!("Could not start local GUI: {e}");
            return Ok(());
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Relay")
            .with_app_id("relay")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/relay.png"))
                    .expect("bundled Relay icon"),
            )
            .with_transparent(false)
            .with_inner_size([1050.0, 780.0])
            .with_min_inner_size([400.0, 400.0]),
        renderer: eframe::Renderer::Glow,
        // Winit's Wayland set_visible is a no-op. X11/XWayland provides real
        // unmap/remap semantics for close-to-tray, without a compositor hack.
        event_loop_builder: Some(Box::new(|builder| {
            if std::env::var_os("DISPLAY").is_some() {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_x11();
            }
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Relay",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, demo, instance)))),
    )
}
